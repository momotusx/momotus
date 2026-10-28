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
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import app.marquee.core.model.Network as NetworkType

/**
 * What the OS says about the app, forwarded to the core as `lifecycle` events and to the image
 * cache: the app moving to the front or back, low memory, and the default network changing.
 */
class AppLifecycle(
    private val app: Application,
    private val session: CoreSession,
    private val sync: SyncController,
    /** Whether the default network is gone, from the network callback's thread. */
    private val onOffline: (Boolean) -> Unit = {},
) {
    fun register() {
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    foreground()
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

    // The answer matters: when the system took the stream server's listener while the app was away, the
    // server took another port, and an open player opens its play again where it was.
    private fun foreground() {
        session.scope.launch {
            val moved =
                try {
                    session.ops.foreground()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    CoreLog.w("foreground: ${e.message}")
                    false
                }
            if (moved) session.serverMoved.emit(Unit)
        }
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
        val default =
            DefaultNetwork { type, metered ->
                session.ops.networkChanged(type, metered)
                onOffline(type == null)
            }
        try {
            connectivity.registerDefaultNetworkCallback(
                object : ConnectivityManager.NetworkCallback() {
                    // Follows onAvailable with the new network's transports; metered from the cost flag, never the
                    // interface (plans/offline-downloads.md 4.13), so a hotspot or a VPN over mobile data is metered.
                    override fun onCapabilitiesChanged(
                        network: Network,
                        caps: NetworkCapabilities,
                    ) = default.seen(
                        network,
                        DeviceCaps.networkType(caps::hasTransport),
                        !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
                    )

                    override fun onBlockedStatusChanged(
                        network: Network,
                        blocked: Boolean,
                    ) = default.blocked(network, blocked)

                    override fun onLost(network: Network) = default.lost(network)
                },
            )
            // No default network at all calls no callback: a start in airplane mode says none itself (code review r1
            // C2).
            if (connectivity.activeNetwork == null) default.noneAtStart()
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
 * with its type and cost, the one current at registration too, and again when its cost changes. A lost
 * network, or one blocked for this app, is sent as none (null). The core starts every run on `other`, so
 * without it Settings' This device and Measure again would read and clear another network's speeds.
 */
internal class DefaultNetwork(
    private val send: (type: NetworkType?, metered: Boolean) -> Unit,
) {
    private var current: Any? = null
    private var type: NetworkType? = null
    private var metered = false
    private var blocked = false
    private var said: Triple<Any?, NetworkType?, Boolean>? = null

    @Synchronized
    fun seen(
        network: Any,
        type: NetworkType,
        metered: Boolean,
    ) {
        if (network != current) blocked = false
        current = network
        this.type = type
        this.metered = metered
        say()
    }

    @Synchronized
    fun blocked(
        network: Any,
        blocked: Boolean,
    ) {
        if (network != current) return
        this.blocked = blocked
        say()
    }

    @Synchronized
    fun lost(network: Any) {
        if (network != current) return
        current = null
        say()
    }

    /** Nothing reported yet and no default network: none, once. */
    @Synchronized
    fun noneAtStart() {
        if (said == null && current == null) say()
    }

    private fun say() {
        val up = current != null && !blocked
        val now = Triple(current.takeIf { up }, type.takeIf { up }, up && metered)
        if (now == said) return
        said = now
        send(now.second, now.third)
    }
}
