package app.marquee.android

import android.app.Application
import android.net.ConnectivityManager
import android.net.Network
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import app.marquee.core.session.LifecycleEvent
import coil3.SingletonImageLoader

/**
 * What the OS says about the app, forwarded to the core as `lifecycle` events and to the image
 * cache: the app moving to the front or back, low memory, and the default network changing.
 */
class AppLifecycle(
    private val app: Application,
    private val session: CoreSession,
) {
    fun register() {
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = session.ops.lifecycle(LifecycleEvent.Foreground)

                override fun onStop(owner: LifecycleOwner) = session.ops.lifecycle(LifecycleEvent.Background)
            },
        )
        watchNetwork()
    }

    /** Called from `Application.onTrimMemory`. */
    fun trimMemory(level: Int) {
        val cache = SingletonImageLoader.get(app).memoryCache
        when {
            level >= BACKGROUND -> cache?.clear()
            level >= RUNNING_CRITICAL -> cache?.let { it.trimToSize(it.size / 2) }
        }
        if (level >= RUNNING_CRITICAL) session.ops.lifecycle(LifecycleEvent.MemoryPressure)
    }

    private fun watchNetwork() {
        val connectivity = app.getSystemService(ConnectivityManager::class.java) ?: return
        var first = true
        try {
            connectivity.registerDefaultNetworkCallback(
                object : ConnectivityManager.NetworkCallback() {
                    override fun onAvailable(network: Network) {
                        // The system reports the current network right at registration; that is not a change.
                        if (first) {
                            first = false
                            return
                        }
                        session.ops.lifecycle(LifecycleEvent.NetworkChanged)
                    }
                },
            )
        } catch (e: SecurityException) {
            CoreLog.w("network changes are not reported: ${e.message}")
        }
    }

    private companion object {
        // ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL and TRIM_MEMORY_BACKGROUND
        const val RUNNING_CRITICAL = 15
        const val BACKGROUND = 40
    }
}
