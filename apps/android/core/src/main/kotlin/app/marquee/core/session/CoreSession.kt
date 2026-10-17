package app.marquee.core.session

import android.content.Context
import app.marquee.core.NativeCore
import app.marquee.core.client.BridgeMetrics
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.client.LifecycleSink
import app.marquee.core.client.NoOpLifecycleSink
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
import kotlin.concurrent.thread

/**
 * The process-wide handle on the Rust core: boots it on a background thread as early as possible,
 * owns the protocol client and the secret store, and creates live views. One per process
 * (the Application holds it); the core object is kept referenced forever, because dropping it
 * stops the core's runtime.
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

    /** Completes when the core is open, or fails with why it could not be. */
    val ready = CompletableDeferred<Unit>()

    @Volatile
    var lifecycle: LifecycleSink = NoOpLifecycleSink

    @Volatile
    var openMillis: Long = -1
        private set

    @Volatile
    private var core: MarqueeCore? = null

    fun version(): String? = core?.version()

    fun start(keepCallbackThreadsAttached: Boolean = true) {
        thread(name = "marquee-boot") {
            try {
                val started = System.nanoTime()
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
                        keepCallbackThreadsAttached,
                    )
                core = opened
                client.attach { text -> opened.send(text) }
                openMillis = (System.nanoTime() - started) / 1_000_000
                CoreLog.d("core open $openMillis ms, version ${opened.version()}")
                ready.complete(Unit)
                restoreSecrets()
            } catch (t: Throwable) {
                CoreLog.e("core failed to open", t)
                // Waiting requests fail with the real reason instead of timing out as "not responding".
                client.attachFailed("The core could not start: ${t.message ?: t.javaClass.simpleName}")
                ready.completeExceptionally(t)
            }
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

    fun pairing(server: String) = PairingView(client, scope, server).also { it.open() }

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
