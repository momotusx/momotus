package app.marquee.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.annotation.VisibleForTesting
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import app.marquee.core.client.CoreLog

/**
 * Keeps the process running while a start goes on with the app away (plans/player-polish.md 2c): a foreground
 * service of the media playback type, as the start is the first part of the viewer's play, with its "Preparing
 * <title>" notification. Without it Android freezes a backgrounded app within seconds, and the core's resolve, the
 * engine and Media3's buffering stop with it. It holds a partial wake lock and a Wi-Fi lock, so a screen turned off
 * does not stop the start either, and ends itself after [LIMIT_MS] whatever happens.
 *
 * [keep] starts it while the player is still in the foreground (the activity pausing), as Android 12 and later
 * allow; [release] ends it at Ready, on a failure, when the viewer is back or leaves the player. Main thread only.
 */
class StartService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null
    private val limit = Runnable { release() }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        val title = intent?.getStringExtra(TITLE).orEmpty()
        // Android stops an app that asked for a foreground service and did not start one: always first.
        ServiceCompat.startForeground(this, NOTIFICATION, notification(title), TYPE)
        if (!wanted) {
            // Released before it got here (the start ended at once).
            end()
            return START_NOT_STICKY
        }
        running = this
        hold()
        handler.removeCallbacks(limit)
        handler.postDelayed(limit, LIMIT_MS)
        return START_NOT_STICKY
    }

    // Swiped away from Recents: nothing is left to prepare for.
    override fun onTaskRemoved(rootIntent: Intent?) = release()

    override fun onDestroy() {
        handler.removeCallbacks(limit)
        wake?.takeIf { it.isHeld }?.release()
        wifi?.takeIf { it.isHeld }?.release()
        if (running === this) running = null
        super.onDestroy()
    }

    private fun hold() {
        if (wake == null) {
            wake =
                getSystemService(PowerManager::class.java)
                    ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "marquee:start")
                    ?.apply { acquire(LIMIT_MS) }
        }
        if (wifi == null) {
            // As Media3 holds it while it streams: Wi-Fi out of power save, also with the screen off.
            @Suppress("DEPRECATION")
            wifi =
                applicationContext
                    .getSystemService(WifiManager::class.java)
                    ?.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "marquee:start")
                    ?.apply {
                        setReferenceCounted(false)
                        acquire()
                    }
        }
    }

    private fun end() {
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun notification(title: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26 && manager?.getNotificationChannel(CHANNEL) == null) {
            manager?.createNotificationChannel(
                NotificationChannel(CHANNEL, "Preparing to play", NotificationManager.IMPORTANCE_LOW).apply {
                    setShowBadge(false)
                },
            )
        }
        // A tap brings the app back, where the player waits.
        val open =
            (
                packageManager.getLaunchIntentForPackage(packageName)
                    ?: packageManager.getLeanbackLaunchIntentForPackage(packageName)
            )?.let {
                PendingIntent.getActivity(
                    this,
                    0,
                    it,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
            }
        return NotificationCompat
            .Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_preparing)
            .setContentTitle(preparingWords(title))
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    companion object {
        /** A start that takes longer has failed or is stuck: the core's whole run has a 3 minute budget. */
        const val LIMIT_MS = 5 * 60_000L

        private const val TITLE = "title"
        private const val CHANNEL = "preparing"
        private const val NOTIFICATION = 7_401
        private val TYPE = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0

        private var wanted = false
        private var running: StartService? = null

        /** Whether a start is kept going in the background now (asked for, and not released since). */
        @get:VisibleForTesting
        val keeping: Boolean get() = wanted

        /** Keeps the start of [title] going while the app is away; refused quietly where Android does not allow it. */
        fun keep(
            context: Context,
            title: String,
        ) {
            wanted = true
            val intent = Intent(context, StartService::class.java).putExtra(TITLE, title)
            try {
                ContextCompat.startForegroundService(context, intent)
            } catch (e: IllegalStateException) {
                // ForegroundServiceStartNotAllowedException (12 and later): the app was already in the background.
                wanted = false
                CoreLog.w("the start cannot go on in the background: ${e.message}")
            }
        }

        /** The start no longer needs the app kept running: it reached Ready or failed, or the viewer is back or left. */
        fun release() {
            wanted = false
            running?.end()
            running = null
        }
    }
}

/** The notification's words while a start goes on in the background. */
fun preparingWords(title: String): String = if (title.isBlank()) "Preparing to play" else "Preparing $title"
