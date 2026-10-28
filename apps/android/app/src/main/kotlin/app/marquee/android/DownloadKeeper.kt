package app.marquee.android

import android.annotation.SuppressLint
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.storage.StorageManager
import androidx.annotation.RequiresApi
import androidx.compose.runtime.snapshotFlow
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import app.marquee.core.DownloadKeeperLogic
import app.marquee.core.KeeperEffect
import app.marquee.core.KeeperEvent
import app.marquee.core.KeeperFacts
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Keeps downloads running when the user leaves the app (plans/offline-downloads.md 5): it follows the `downloads`
 * view for the life of the process and performs what [DownloadKeeperLogic] says: a user-initiated data transfer job
 * on Android 14 and later, a `dataSync` foreground service below, the progress notification, `downloadsAllowed`.
 * The job and the service only keep the process; the core runs the downloads. Main thread only.
 */
class DownloadKeeper(
    private val app: Application,
    private val session: CoreSession,
) {
    private val logic = DownloadKeeperLogic()
    private var progress: KeeperEffect.Progress? = null
    private var job: Pair<JobService, JobParameters>? = null
    private var allowed = true

    /** The failure notices up, so a profile change takes them down. */
    private val failures = mutableSetOf<Int>()

    fun start() {
        createChannel()
        session.scope.launch(Dispatchers.Main) {
            val view = session.downloads()
            snapshotFlow { view.snapshot }.filterNotNull().collect { on(KeeperEvent.Facts(KeeperFacts.of(it))) }
        }
        session.scope.launch(Dispatchers.Main) {
            while (true) {
                delay(MINUTE_MS)
                on(KeeperEvent.Minute)
            }
        }
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = on(KeeperEvent.Visible(true))

                override fun onStop(owner: LifecycleOwner) = on(KeeperEvent.Visible(false))
            },
        )
    }

    fun on(event: KeeperEvent) = logic.on(event).forEach(::perform)

    private fun perform(effect: KeeperEffect) {
        when (effect) {
            is KeeperEffect.Keep -> {
                keep(effect.bytes, effect.unmetered)
            }

            is KeeperEffect.Progress -> {
                show(effect)
            }

            KeeperEffect.Release -> {
                release()
            }

            KeeperEffect.HideProgress -> {
                if (job == null) NotificationManagerCompat.from(app).cancel(PROGRESS_ID)
            }

            KeeperEffect.ClearNotices -> {
                val shade = NotificationManagerCompat.from(app)
                shade.cancel(FINISHED_ID)
                failures.forEach(shade::cancel)
                failures.clear()
            }

            KeeperEffect.SettleLater -> {
                session.scope.launch(Dispatchers.Main) {
                    delay(SETTLE_MS)
                    on(KeeperEvent.Settled)
                }
            }

            is KeeperEffect.Finished -> {
                notice(FINISHED_ID, effect.title, effect.text)
            }

            is KeeperEffect.Failed -> {
                val id = effect.id.hashCode()
                failures += id
                notice(id, "Couldn't download ${effect.title}", null)
            }

            is KeeperEffect.Allowed -> {
                allowed = effect.allowed
                session.ops.downloadsAllowed(effect.allowed)
            }

            is KeeperEffect.Allocate -> {
                allocate(effect.bytes)
            }
        }
    }

    private fun keep(
        bytes: Long,
        unmetered: Boolean,
    ) {
        try {
            if (Build.VERSION.SDK_INT >=
                34
            ) {
                schedule(bytes, unmetered)
            } else {
                ContextCompat.startForegroundService(app, serviceIntent())
            }
        } catch (e: RuntimeException) {
            // Refused (the app left the front between the tap and here): downloads go on while the process lives,
            // and the next minute asks again.
            CoreLog.w("downloads cannot be kept running: ${e.message}")
            session.scope.launch(Dispatchers.Main) { on(KeeperEvent.Refused) }
        }
    }

    /**
     * The user-initiated job, with Wi-Fi as its network when every download waits for it: the system starts it when
     * Wi-Fi comes, the app away (code review r1 C1). A refused schedule (the quota, the app not in front) lets go of
     * the hold and is asked again a minute later, not at every snapshot (M2, r2 n3). The same id replaces a job
     * whose network changed (r2 N2).
     */
    @RequiresApi(34)
    private fun schedule(
        bytes: Long,
        unmetered: Boolean,
    ) {
        val network = if (unmetered) JobInfo.NETWORK_TYPE_UNMETERED else JobInfo.NETWORK_TYPE_ANY
        val info =
            JobInfo
                .Builder(JOB_ID, ComponentName(app, DownloadJob::class.java))
                .setUserInitiated(true)
                .setRequiredNetworkType(network)
                .setEstimatedNetworkBytes(if (bytes > 0) bytes else JobInfo.NETWORK_BYTES_UNKNOWN.toLong(), 0)
                .build()
        val scheduled = app.getSystemService(JobScheduler::class.java)?.schedule(info)
        if (scheduled != JobScheduler.RESULT_SUCCESS) {
            CoreLog.w("the downloads job was not scheduled: $scheduled")
            session.scope.launch(Dispatchers.Main) { on(KeeperEvent.Refused) }
        }
    }

    private fun release() {
        progress = null
        val running = job
        job = null
        if (running != null) {
            running.first.jobFinished(running.second, false)
        } else if (Build.VERSION.SDK_INT >= 34) {
            app.getSystemService(JobScheduler::class.java)?.cancel(JOB_ID)
        }
        app.stopService(serviceIntent())
        NotificationManagerCompat.from(app).cancel(PROGRESS_ID)
    }

    /** The job started: it shows the notification at once, as the system asks of a user-initiated job. */
    @RequiresApi(34)
    fun jobStarted(
        service: JobService,
        params: JobParameters,
    ) {
        job = service to params
        service.setNotification(params, PROGRESS_ID, notification(), JobService.JOB_END_NOTIFICATION_POLICY_REMOVE)
        on(KeeperEvent.Started)
    }

    fun jobStopped(params: JobParameters) {
        if (job?.second === params) job = null
        // The keeper's own replace (a new network) or release stopped it: the hold stands or is already let go.
        if (Build.VERSION.SDK_INT >= 31 && params.stopReason == JobParameters.STOP_REASON_CANCELLED_BY_APP) return
        on(KeeperEvent.Stopped)
    }

    @SuppressLint("MissingPermission")
    private fun show(effect: KeeperEffect.Progress) {
        progress = effect
        val running = job
        if (running != null && Build.VERSION.SDK_INT >= 34) {
            running.first.setNotification(
                running.second,
                PROGRESS_ID,
                notification(),
                JobService.JOB_END_NOTIFICATION_POLICY_REMOVE,
            )
        } else if (permitted()) {
            NotificationManagerCompat.from(app).notify(PROGRESS_ID, notification())
        }
    }

    /** The progress notification as it stands; the service and the job show it at their start. */
    fun notification(): Notification {
        val now = progress
        val percent = now?.fraction?.let { (it * 100).toInt() }
        return NotificationCompat
            .Builder(app, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_download)
            .setContentTitle(now?.title ?: "Downloading")
            .setContentText(now?.text)
            .setProgress(100, percent ?: 0, percent == null)
            .setContentIntent(open())
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    @SuppressLint("MissingPermission")
    private fun notice(
        id: Int,
        title: String,
        text: String?,
    ) {
        if (!permitted()) return
        val notice =
            NotificationCompat
                .Builder(app, CHANNEL)
                .setSmallIcon(R.drawable.ic_stat_download)
                .setContentTitle(title)
                .setContentText(text)
                .setContentIntent(open())
                .setAutoCancel(true)
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .build()
        NotificationManagerCompat.from(app).notify(id, notice)
    }

    private fun permitted() = NotificationManagerCompat.from(app).areNotificationsEnabled()

    private fun open(): PendingIntent? =
        app.packageManager.getLaunchIntentForPackage(app.packageName)?.let {
            PendingIntent.getActivity(app, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT < 26) return
        app.getSystemService(NotificationManager::class.java)?.createNotificationChannel(
            NotificationChannel(CHANNEL, "Downloads", NotificationManager.IMPORTANCE_LOW).apply { setShowBadge(false) },
        )
    }

    /** A download waits for space: the system may free its caches for it (4.9); the core then looks again. */
    private fun allocate(bytes: Long) {
        if (Build.VERSION.SDK_INT < 26) return
        session.scope.launch(Dispatchers.IO) {
            try {
                val storage = app.getSystemService(StorageManager::class.java) ?: return@launch
                val volume = storage.getUuidForPath(app.filesDir)
                storage.allocateBytes(volume, minOf(bytes, storage.getAllocatableBytes(volume)))
            } catch (e: Exception) {
                CoreLog.w("no space could be freed for downloads: ${e.message}")
            }
            withContext(Dispatchers.Main) { session.ops.downloadsAllowed(allowed) }
        }
    }

    private fun serviceIntent() = Intent(app, DownloadService::class.java)

    private companion object {
        const val CHANNEL = "downloads"
        const val JOB_ID = 7_501
        const val PROGRESS_ID = 7_502
        const val FINISHED_ID = 7_503
        const val MINUTE_MS = 60_000L
        const val SETTLE_MS = 5_000L
    }
}

private val Context.keeper: DownloadKeeper get() = (applicationContext as MarqueeApp).downloadKeeper

/** Android 14 and later: the user-initiated data transfer job that keeps the process while downloads run. */
class DownloadJob : JobService() {
    override fun onStartJob(params: JobParameters): Boolean {
        if (Build.VERSION.SDK_INT >= 34) keeper.jobStarted(this, params)
        return true
    }

    // The system stopped it (the network went, a quota): it may run it again; the keeper hears when.
    override fun onStopJob(params: JobParameters): Boolean {
        keeper.jobStopped(params)
        return true
    }
}

/** Android 7 to 13: a foreground service of the `dataSync` type with the same notification. */
class DownloadService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        // Android stops an app that asked for a foreground service and did not start one: always first.
        val type = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0
        ServiceCompat.startForeground(this, NOTIFICATION, keeper.notification(), type)
        return START_NOT_STICKY
    }

    private companion object {
        /** The keeper's progress notification, so the service takes it over. */
        const val NOTIFICATION = 7_502
    }
}
