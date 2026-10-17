package app.marquee.core.session

import android.content.Context
import android.content.pm.ApplicationInfo
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.NativeCore
import app.marquee.core.client.BridgeMetrics
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.ffi.CoreListener
import app.marquee.core.ffi.MarqueeCore
import app.marquee.core.secrets.DeviceId
import app.marquee.core.secrets.KeystoreKeySource
import app.marquee.core.secrets.SecretStore
import app.marquee.core.views.AddonsView
import app.marquee.core.views.DiscoverView
import app.marquee.core.views.HomeView
import app.marquee.core.views.ImportView
import app.marquee.core.views.LibraryView
import app.marquee.core.views.PairingView
import app.marquee.core.views.PlaybackView
import app.marquee.core.views.SearchView
import app.marquee.core.views.SettingsView
import app.marquee.core.views.TitleView
import app.marquee.core.views.VersionsView
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.File
import java.util.concurrent.Executors
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
    private val stateThread = Executors.newSingleThreadExecutor { Thread(it, "marquee-state") }
    val client = CoreClient(stateThread.asCoroutineDispatcher(), CoreJson, metrics)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    val ops = CoreOps(client)
    val secrets = SecretStore(File(context.noBackupFilesDir, "secrets.bin"), KeystoreKeySource())

    /** Completes when the core first opens, or fails with why it could not. */
    val ready = CompletableDeferred<Unit>()

    /** What the UI shows when the core is not running. Written by the boot and state threads. */
    var health: CoreHealth by mutableStateOf(CoreHealth.Starting)
        private set

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
                client.attach { text -> opened.send(text) }
                openMillis = (System.nanoTime() - started) / 1_000_000
                CoreLog.d("core open $openMillis ms, version ${opened.version()}")
                // A core that died again while this one was opening keeps the state its death set.
                if (client.epoch == deaths) health = CoreHealth.Running
                ready.complete(Unit)
                restoreSecrets()
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

    /** The core keeps no secrets: hand it the debrid accounts again at every start, with one retry. */
    private fun restoreSecrets() {
        val keys = secrets.load().debridKeys
        if (keys.isEmpty()) return
        scope.launch {
            for (attempt in 1..2) {
                try {
                    ops.setDebridKeys(keys)
                    return@launch
                } catch (e: Exception) {
                    CoreLog.w("debrid keys not applied (attempt $attempt): ${CoreLog.describe(e)}")
                    delay(2_000)
                }
            }
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

    fun addons() = AddonsView(client, scope).also { it.open() }

    fun settings() = SettingsView(client, scope).also { it.open() }

    fun import() = ImportView(client, scope).also { it.open() }

    fun pairing(
        server: String,
        name: String,
    ) = PairingView(client, scope, server, name).also { it.open() }

    /**
     * Ranked stream choices for one video. `title` is the movie or series name, never an episode
     * title (ranking uses it to hide wrong content); `season` and `episode` come from the
     * selected episode or from the video id.
     */
    fun versions(
        kind: String,
        videoId: String,
        title: String,
        caps: DeviceCapsParams,
        season: Int? = null,
        episode: Int? = null,
        bingeGroup: String? = null,
    ): VersionsView {
        val params = HashMap<String, kotlinx.serialization.json.JsonElement>()
        params["kind"] = JsonPrimitive(kind)
        params["videoId"] = JsonPrimitive(videoId)
        params["title"] = JsonPrimitive(title)
        season?.let { params["season"] = JsonPrimitive(it) }
        episode?.let { params["episode"] = JsonPrimitive(it) }
        bingeGroup?.let { params["bingeGroup"] = JsonPrimitive(it) }
        params.putAll(caps.toJson())
        return VersionsView(client, scope, JsonObject(params)).also { it.open() }
    }

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
            it.opened.await()
        }
    }
}
