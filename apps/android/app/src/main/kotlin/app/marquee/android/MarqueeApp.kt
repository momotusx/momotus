package app.marquee.android

import android.app.Application
import android.util.Log
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession

class MarqueeApp : Application() {
    lateinit var session: CoreSession
        private set

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
    }

    companion object {
        const val TAG = "marquee"
    }
}
