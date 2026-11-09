package app.marquee.core.session

import android.content.Context
import android.content.pm.ApplicationInfo
import androidx.annotation.VisibleForTesting
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.NativeCore
import app.marquee.core.client.BridgeMetrics
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.client.StateThread
import app.marquee.core.client.TitleFacts
import app.marquee.core.client.versionsParams
import app.marquee.core.ffi.CoreListener
import app.marquee.core.ffi.MarqueeCore
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.model.TraktStatus
import app.marquee.core.secrets.DebridCore
import app.marquee.core.secrets.DeviceId
import app.marquee.core.secrets.KeystoreKeySource
import app.marquee.core.secrets.SecretDebridKeystore
import app.marquee.core.secrets.SecretStore
import app.marquee.core.views.AddonCatalogView
import app.marquee.core.views.AddonsView
import app.marquee.core.views.CalendarView
import app.marquee.core.views.DebridLoginView
import app.marquee.core.views.DebridView
import app.marquee.core.views.DiscoverView
import app.marquee.core.views.DownloadsView
import app.marquee.core.views.HomeView
import app.marquee.core.views.ImportView
import app.marquee.core.views.LibraryView
import app.marquee.core.views.PairingView
import app.marquee.core.views.PlaybackView
import app.marquee.core.views.ProfilesView
import app.marquee.core.views.SearchView
import app.marquee.core.views.SettingsView
import app.marquee.core.views.SignInView
import app.marquee.core.views.TitleView
import app.marquee.core.views.TraktLoginView
import app.marquee.core.views.TraktView
import app.marquee.core.views.VersionsView
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/** Whether the core is running. The UI shows [Stopped] with a way to try again. */
sealed interface CoreHealth {
    data object Starting : CoreHealth

    data object Running : CoreHealth

    /** The core stopped and the session is opening a new one. */
    data object Restarting : CoreHealth

    /** [reason] is for the log and diagnostics; it can quote a panic message. */
    data class Stopped(
        val reason: String,
    ) : CoreHealth
}

/**
 * The process-wide handle on the Rust core: boots it on a background thread as early as possible,
 * owns the protocol client and the secret store, and creates live views. One per process
 * (the Application holds it); the core object is kept referenced, because dropping it stops the
 * core's runtime. When the core reports that it died, the session opens a fresh one and the
 * client brings the open views back on it ([CoreClient.attach]).
 */
class CoreSession(
    private val context: Context,
) {
    val metrics = BridgeMetrics()

    // One looped thread of its own; nothing but its dispatcher posts there (StateThread).
    val client = CoreClient(StateThread("marquee-state").dispatcher, CoreJson, metrics)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    val ops = CoreOps(client)
    val secrets = SecretStore(File(context.noBackupFilesDir, "secrets.bin"), KeystoreKeySource())

    /** The device's debrid accounts: restored on every core open, before any screen opens a view. */
    val debridAccounts =
        DebridAccounts(
            SecretDebridKeystore(secrets),
            object : DebridCore {
                override suspend fun restore(
                    records: List<DebridRecord>,
                    tombstones: List<DebridTombstone>,
                ) = ops.restoreDebrid(records, tombstones)

                override suspend fun ack(
                    service: String,
                    generation: String,
                    revision: Long,
                ) = ops.ackDebrid(service, generation, revision)

                override suspend fun disconnect(
                    service: String,
                    generation: String,
                ) = ops.disconnectDebrid(service, generation)
            },
            connect = { service, key -> ops.setDebridKey(service, key) },
            scope,
        )

    /** The `debrid` view for the app's life: the accounts for Settings, the pending records for the keystore. */
    val debrid: DebridView by lazy { DebridView(client, scope, debridAccounts::pushed).also { it.open() } }

    /** Completes when the core first opens, or fails with why it could not. */
    val ready = CompletableDeferred<Unit>()

    /** What the open core says it carries: the torrent engine (mq-ffi's `has_engine()`); true until it has opened. */
    @Volatile
    var hasEngine: Boolean = true
        private set

    /**
     * Returns once the core has first opened, or throws why it could not: the start decision's wait. It waits
     * on [ready], never on a read of [health]. That read can start inside a composition's snapshot, which still
     * holds the value from before the boot thread's write, and the write's apply notification can go out before
     * the read listens: the change is then never seen.
     */
    suspend fun awaitOpen() = ready.await()

    /**
     * The boot thread's last step: [running] when the core did not die while it opened. Only a running core
     * lets the start decide; one that died is opened again, or the start hears it gave up ([onCoreDied]).
     */
    @VisibleForTesting
    fun opened(running: Boolean) {
        if (!running) return
        health = CoreHealth.Running
        ready.complete(Unit)
    }

    /**
     * The stream server took another port while the app was away (the `foreground` answer): a player
     * with a loaded item opens its play again, where it was.
     */
    val serverMoved = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    private val healthState = mutableStateOf<CoreHealth>(CoreHealth.Starting)

    /**
     * What the UI shows when the core is not running. Written by the boot and state threads, so each write sends its
     * own apply notification: Compose sends them only once a composition runs, and a wait that listens from the app's
     * start (the profiles gate) would otherwise never hear the core open (a fresh install sat on Starting, bench PGA12).
     */
    var health: CoreHealth
        get() = healthState.value
        private set(value) {
            healthState.value = value
            Snapshot.sendApplyNotifications()
        }

    @Volatile
    var openMillis: Long = -1
        private set

    @Volatile
    private var core: MarqueeCore? = null
    private val debuggable = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
    private var keepAttached = true
    private val booting = AtomicBoolean(false)
    private val restartTimes = ArrayDeque<Long>()

    fun version(): String? = core?.version()

    /** Runs on the boot thread right after each core is attached, before anything else is asked of it. */
    @Volatile
    var onAttached: (() -> Unit)? = null

    fun start(keepCallbackThreadsAttached: Boolean = true) {
        keepAttached = keepCallbackThreadsAttached
        client.onCoreDied = ::onCoreDied
        boot()
    }

    /** Opens the core again after it stopped. Does nothing while a boot is already running. */
    fun restart() = boot(announce = true)

    private fun boot(announce: Boolean = false) {
        if (!booting.compareAndSet(false, true)) return
        if (announce) health = CoreHealth.Restarting
        thread(name = "marquee-boot") {
            try {
                val started = System.nanoTime()
                // Each step's time, so a slow start says which one waited.
                CoreLog.d("core opening")
                val deaths = client.epoch
                // The dead core holds the database and its runtime; drop it before opening a new one.
                core?.close()
                core = null
                val listener =
                    object : CoreListener {
                        override fun onMessage(json: String) = client.onMessage(json)
                    }
                val opened =
                    NativeCore.open(
                        context,
                        File(context.filesDir, "marquee"),
                        DeviceId.load(context.noBackupFilesDir),
                        listener,
                        keepAttached,
                        // Debug builds log the core at debug level, release builds keep its default.
                        logFilter = if (debuggable) "debug" else null,
                    )
                core = opened
                hasEngine =
                    app.marquee.core.ffi
                        .hasEngine()
                val native = (System.nanoTime() - started) / 1_000_000
                // Pushes from a revived `debrid` view wait for this core's restore.
                debridAccounts.pause()
                client.attach { text -> opened.send(text) }
                // Each core this process opens hears who the app is (error reports, plans/error-reports.md 5.1).
                onAttached?.invoke()
                // Accounts come back before anything may play (plans/c10-handoff.md, phase 3a).
                runBlocking { debridAccounts.opened { debrid.snapshot?.pending } }
                openMillis = (System.nanoTime() - started) / 1_000_000
                CoreLog.d(
                    "core open $openMillis ms (native open $native ms, then accounts), version ${opened.version()}",
                )
                // A core that died again while this one was opening keeps the state its death set.
                opened(running = client.epoch == deaths)
            } catch (t: Throwable) {
                CoreLog.e("core failed to open", t)
                val reason = "The core could not start: ${t.message ?: t.javaClass.simpleName}"
                // Waiting requests fail with the real reason instead of timing out as "not responding".
                client.attachFailed(reason)
                health = CoreHealth.Stopped(reason)
                ready.completeExceptionally(t)
            } finally {
                booting.set(false)
            }
        }
    }

    /** The core's request loop ended by a panic: say so, and open it again unless it keeps dying. */
    private fun onCoreDied(reason: String) {
        if (!mayRestart()) {
            health = CoreHealth.Stopped(reason)
            // A start still waiting goes Home, which says why.
            ready.completeExceptionally(CoreError(reason, kind = "coreStopped"))
            return
        }
        health = CoreHealth.Restarting
        scope.launch {
            delay(500)
            restart()
        }
    }

    /** At most three automatic restarts in a minute; after that the user decides. */
    private fun mayRestart(): Boolean {
        val now = System.nanoTime() / 1_000_000
        synchronized(restartTimes) {
            while (restartTimes.isNotEmpty() && now - restartTimes.first() > 60_000) restartTimes.removeFirst()
            if (restartTimes.size >= 3) return false
            restartTimes.addLast(now)
            return true
        }
    }

    fun home() = HomeView(client, scope).also { it.open() }

    fun title(
        kind: String,
        id: String,
    ) = TitleView(client, scope, kind, id).also { it.open() }

    fun search() = SearchView(client, scope).also { it.open() }

    fun discover() = DiscoverView(client, scope).also { it.open() }

    fun library() = LibraryView(client, scope).also { it.open() }

    fun downloads() = DownloadsView(client, scope).also { it.open() }

    /** [today] is the local date, `YYYY-MM-DD`. */
    fun calendar(today: String) = CalendarView(client, scope, today).also { it.open() }

    fun addons() = AddonsView(client, scope).also { it.open() }

    fun settings() = SettingsView(client, scope).also { it.open() }

    fun addonCatalog(fresh: Boolean = false) = AddonCatalogView(client, scope, fresh).also { it.open() }

    fun import() = ImportView(client, scope).also { it.open() }

    fun profiles() = ProfilesView(client, scope).also { it.open() }

    fun debridLogin(service: String) = DebridLoginView(client, scope, service).also { it.open() }

    fun trakt(onStatus: (TraktStatus) -> Unit) = TraktView(client, scope, onStatus).also { it.open() }

    fun traktLogin() = TraktLoginView(client, scope).also { it.open() }

    fun pairing(
        server: String,
        name: String,
        form: String? = null,
    ) = PairingView(client, scope, server, name, form).also { it.open() }

    /** A brokered sign-in, or Apple's revoke with [revoke]; every phase reaches [onPhase] as it arrives. */
    fun signIn(
        server: String,
        provider: String,
        revoke: Boolean = false,
        onPhase: (SignInSnapshot) -> Unit,
    ) = SignInView(client, scope, server, provider, revoke, onPhase).also { it.open() }

    /**
     * Ranked stream choices for one video. `title` is the movie or series name, never an episode
     * title (ranking uses it to hide wrong content); `season` and `episode` come from the
     * selected episode or from the video id. [itemId] is the title's id. [download] ranks the versions to
     * keep for a download instead (`purpose: "download"`, plans/offline-downloads.md 4.5).
     */
    fun versions(
        kind: String,
        itemId: String,
        videoId: String,
        title: String,
        caps: DeviceCapsParams,
        season: Int? = null,
        episode: Int? = null,
        bingeGroup: String? = null,
        facts: TitleFacts? = null,
        download: Boolean = false,
    ): VersionsView =
        VersionsView(
            client,
            scope,
            versionsParams(kind, itemId, videoId, title, caps, season, episode, bingeGroup, facts, download),
        ).also { it.open() }

    /** Starts playback of one row of an open Versions view; the Versions view must stay open until this one is. */
    suspend fun playback(
        versions: VersionsView,
        key: String,
        kind: String,
        itemId: String,
        videoId: String,
        name: String?,
        poster: String?,
        season: Int?,
        episode: Int?,
    ): PlaybackView {
        // The Versions view may have subscribed again since it opened; use its subscription now.
        val versionsSub = (versions.currentSubscription ?: versions.opened.await()).sub
        val params = HashMap<String, kotlinx.serialization.json.JsonElement>()
        params["versionsSub"] = JsonPrimitive(versionsSub)
        params["key"] = JsonPrimitive(key)
        params["kind"] = JsonPrimitive(kind)
        params["itemId"] = JsonPrimitive(itemId)
        params["videoId"] = JsonPrimitive(videoId)
        name?.let { params["name"] = JsonPrimitive(it) }
        poster?.let { params["poster"] = JsonPrimitive(it) }
        season?.let { params["season"] = JsonPrimitive(it) }
        episode?.let { params["episode"] = JsonPrimitive(it) }
        // Return once the core has taken the playback subscription, so the caller can close
        // Versions without its unsubscribe overtaking this subscribe.
        return PlaybackView(client, scope, JsonObject(params)).also {
            it.open()
            try {
                it.opened.await()
            } catch (e: Throwable) {
                // The caller left (or the core said no) while the subscribe was in flight: nobody holds this
                // view, so close it here or the core keeps resolving a source for no one.
                it.close()
                throw e
            }
        }
    }
}
