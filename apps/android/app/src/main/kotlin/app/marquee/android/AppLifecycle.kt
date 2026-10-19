package app.marquee.android

import android.app.Application
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import app.marquee.core.session.LifecycleEvent
import coil3.SingletonImageLoader
import app.marquee.core.model.Network as NetworkType

/**
 * What the OS says about the app, forwarded to the core as `lifecycle` events and to the image
 * cache: the app moving to the front or back, low memory, and the default network changing.
 */
class AppLifecycle(
    private val app: Application,
    private val session: CoreSession,
    private val sync: SyncController,
) {
    fun register() {
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    session.ops.lifecycle(LifecycleEvent.Foreground)
                    sync.foreground()
                }

                override fun onStop(owner: LifecycleOwner) {
                    session.ops.lifecycle(LifecycleEvent.Background)
                    sync.background()
                }
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
        val default = DefaultNetwork { session.ops.lifecycle(LifecycleEvent.NetworkChanged, it) }
        try {
            connectivity.registerDefaultNetworkCallback(
                object : ConnectivityManager.NetworkCallback() {
                    // Follows onAvailable with the new network's transports.
                    override fun onCapabilitiesChanged(
                        network: Network,
                        caps: NetworkCapabilities,
                    ) = default.seen(network, DeviceCaps.networkType(caps::hasTransport))
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

/**
 * The default network as the callbacks report it, on their one thread: each new network is sent once
 * with its type, the one current at registration too. The core starts every run on `other`, so without
 * it Settings' This device and Measure again would read and clear another network's speeds.
 */
internal class DefaultNetwork(
    private val send: (NetworkType) -> Unit,
) {
    private var current: Any? = null

    fun seen(
        network: Any,
        type: NetworkType,
    ) {
        if (network == current) return
        current = network
        send(type)
    }
}
