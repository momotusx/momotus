package app.marquee.android

import android.app.Application
import android.util.Log
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import app.marquee.design.createImageLoader
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader

class MarqueeApp :
    Application(),
    SingletonImageLoader.Factory {
    lateinit var session: CoreSession
        private set
    private lateinit var lifecycle: AppLifecycle

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (::lifecycle.isInitialized) lifecycle.trimMemory(level)
    }

    override fun onCreate() {
        super.onCreate()
        CoreLog.sink = { level, message, error ->
            when (level) {
                'e' -> Log.e(TAG, message, error)
                'w' -> Log.w(TAG, message, error)
                else -> Log.d(TAG, message)
            }
        }
        // Open the core on a boot thread right away; the first frame does not wait for it.
        session = CoreSession(this).also { it.start() }
        lifecycle = AppLifecycle(this, session).also { it.register() }
    }

    override fun newImageLoader(context: PlatformContext): ImageLoader = createImageLoader(context)

    companion object {
        const val TAG = "marquee"
    }
}
