package app.marquee.android

import android.app.Application
import android.content.pm.ApplicationInfo
import android.util.Log
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.session.CoreSession
import app.marquee.design.createImageLoader
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.withContext

/** What this device can play right now (see [MarqueeApp.deviceCaps]). */
typealias CapsSource = suspend () -> DeviceCapsParams

class MarqueeApp :
    Application(),
    SingletonImageLoader.Factory {
    lateinit var session: CoreSession
        private set
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
        // Open the core on a boot thread right away; the first frame does not wait for it.
        session = CoreSession(this).also { it.start() }
        lifecycle = AppLifecycle(this, session).also { it.register() }
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
