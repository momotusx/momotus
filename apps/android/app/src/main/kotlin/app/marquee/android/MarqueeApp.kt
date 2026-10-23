package app.marquee.android

import android.app.Application
import android.content.pm.ApplicationInfo
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.session.CoreSession
import app.marquee.core.trakt.SecretTraktRecords
import app.marquee.core.trakt.TraktAccount
import app.marquee.design.createImageLoader
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** What this device can play right now (see [MarqueeApp.deviceCaps]). */
typealias CapsSource = suspend () -> DeviceCapsParams

class MarqueeApp :
    Application(),
    SingletonImageLoader.Factory {
    lateinit var session: CoreSession
        private set

    /** The sync account on this device, for the app's life. */
    lateinit var sync: SyncController

    /** The session's profile and whether profile content may show (plans/m11-profiles.md). */
    lateinit var profiles: ProfileGate
        private set

    /** The running profile's Trakt account (plans/m7-trakt.md): its record, status and view. */
    lateinit var trakt: TraktAccount
        private set

    /** A stremio:// install link the activity was opened with, until the navigation takes it to the link field. */
    var addonLink: String? by mutableStateOf(null)

    /** Credential Manager, Auth Tab and the sign-in's return (plans/accounts.md 4.4), for the app's life. */
    val accountPlatform by lazy { AccountPlatform(this) }

    /** The account flows: the sign-in under way, Account's actions, D13's removal. */
    val accounts by lazy { AccountFlows(this) }

    /**
     * Sign in another device was asked for (the app shortcut, Account, or a phone camera on a TV's QR, which opens
     * the `/pair` App Link): the scanner opens, after sign-in when this device has no account yet. [scanAgain]
     * says why the code must be scanned once more here.
     */
    var pendingScan: Boolean by mutableStateOf(false)
    var scanAgain: Boolean by mutableStateOf(false)
    private lateinit var lifecycle: AppLifecycle

    /**
     * What this device can play, read for each Versions view: the codec list is scanned once off the
     * main thread, the display, receiver and language are read fresh. A failed read means a plain
     * 1080p stick.
     */
    lateinit var deviceCaps: CapsSource
        private set

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (::lifecycle.isInitialized) lifecycle.trimMemory(level)
    }

    override fun onCreate() {
        super.onCreate()
        val debuggable = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        CoreLog.sink = { level, message, error ->
            when (level) {
                'e' -> Log.e(TAG, message, error)

                'w' -> Log.w(TAG, message, error)

                // Debug lines name titles and ids: they stay out of release logs.
                else -> if (debuggable) Log.d(TAG, message)
            }
        }
        // Debug builds only: local fakes (Trakt, Find addons' lists), read by a dev build of the core library.
        DebugEntry.devFakes(this)
        // Open the core on a boot thread right away; the first frame does not wait for it.
        session = CoreSession(this).also { it.start() }
        // An own build whose core has no engine tells the owner (CoreBuild), never the viewer in words.
        session.scope.launch {
            if (runCatching { session.ready.await() }.isSuccess) {
                CoreBuild.check(BuildConfig.CORE_BUILD, session.hasEngine, debuggable)
            }
        }
        sync = SyncController(session, this).also { it.start() }
        profiles = ProfileGate(session, beforeSwitch = { trakt.quiet() }).also { it.start() }
        trakt =
            TraktAccount(session.ops, SecretTraktRecords(session.secrets), session.scope) { push ->
                val view = session.trakt(push)
                AutoCloseable { view.close() }
            }
        // A profile removed here or by sync takes its Trakt sign-in with it.
        session.client.onProfileGone = trakt::forget
        // Each time a profile's content may show (the core opened, unlocked, switched), its record goes to
        // the core and its `trakt` view opens; a shut gate (a switch, a lock, a core that stopped) closes it.
        session.scope.launch(Dispatchers.Main) {
            snapshotFlow { profiles.snapshot?.active?.takeIf { profiles.open } }.collect { active ->
                if (active == null) trakt.end() else trakt.begin(active)
            }
        }
        lifecycle = AppLifecycle(this, session, sync).also { it.register() }
        val scan = session.scope.async(Dispatchers.Default) { DeviceCaps.scanCodecs() }
        deviceCaps = {
            runCatching {
                withContext(
                    Dispatchers.Default,
                ) { DeviceCaps.read(this@MarqueeApp, scan.await()) }
            }.getOrElse {
                if (it is CancellationException) throw it
                CoreLog.w("device capabilities could not be read: ${CoreLog.describe(it)}")
                DeviceCaps.baseline()
            }
        }
    }

    override fun newImageLoader(context: PlatformContext): ImageLoader = createImageLoader(context)

    companion object {
        const val TAG = "marquee"
    }
}
