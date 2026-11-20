package app.marquee.android

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.IntentCompat
import androidx.core.content.pm.PackageInfoCompat
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import app.marquee.core.UpdateFeed
import app.marquee.core.UpdateRules
import app.marquee.core.client.CoreLog
import app.marquee.player.PlayerHost
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlin.random.Random

/**
 * Updates from GitHub Releases (plans/auto-update.md section 1): the signed feed at [feedUrl] once at start and every
 * day after, the APK of this ABI downloaded, checked and staged in a PackageInstaller session, then committed. A silent
 * install waits until the app is in the background with nothing playing or downloading; one that needs the system's
 * dialog waits for the user's Install. Main thread only, except the network and file work it moves off.
 */
class Updater(
    private val app: MarqueeApp,
    private val feedUrl: String,
    keyPem: String,
) {
    sealed interface State {
        data object Idle : State

        data object Checking : State

        data object Current : State

        data class Downloading(
            val version: String,
            val percent: Int?,
        ) : State

        /** Staged; [silent] when no press is needed (UpdateRules.silent), else the system's dialog asks. */
        data class Ready(
            val version: String,
            val silent: Boolean,
        ) : State

        /** Shown only after a check the user asked for. */
        data object Failed : State
    }

    var state: State by mutableStateOf(State.Idle)
        private set

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val key = UpdateFeed.publicKey(keyPem)
    private val pm = app.packageManager
    private val pkg = app.packageName
    private val dir = File(app.noBackupFilesDir, "update")
    private var busy = false
    private var asked = false
    private var offer: UpdateFeed.Offer? = null
    private var staged: File? = null
    private var session = NO_SESSION
    private var committing = false

    /** The system's Install dialog for the committed session, from STATUS_PENDING_USER_ACTION. */
    private var confirm: Intent? = null
    private var visible = false

    fun start() {
        // Sessions from an earlier run never committed (or wait on a dialog no one sees): start clean.
        runCatching { pm.packageInstaller.mySessions.forEach { pm.packageInstaller.abandonSession(it.sessionId) } }
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    visible = true
                    // The toggle may have changed meanwhile; a dialog the system already asked for stays a press.
                    (state as? State.Ready)?.let { if (confirm == null) state = it.copy(silent = silent()) }
                }

                override fun onStop(owner: LifecycleOwner) {
                    visible = false
                    val ready = state as? State.Ready ?: return
                    if (ready.silent && UpdateRules.mayCommitInBackground(playing(), downloading())) commit()
                }
            },
        )
        scope.launch {
            while (true) {
                run()
                delay(DAY_MS + Random.nextLong(SPREAD_MS))
            }
        }
    }

    /** Settings' Check for updates: a run under way takes it over, so its end shows the answer. */
    fun check() {
        if (state is State.Ready || state is State.Downloading) return
        asked = true
        state = State.Checking
        if (!busy) scope.launch { run() }
    }

    /** Settings' Install and Install now: the system's dialog when it asked for one, else the commit itself. */
    fun install() {
        val waiting = confirm
        if (waiting != null) {
            open(waiting)
            return
        }
        commit(asked = true)
    }

    /** One check, download and stage; [asked] (Check for updates) lets it use a metered network and show its answer. */
    private suspend fun run() {
        if (busy || state is State.Ready) return
        busy = true
        try {
            step()
        } finally {
            busy = false
            asked = false
        }
    }

    private suspend fun step() {
        if (!asked && playing()) return
        val installed = installedCode()
        val found = withContext(Dispatchers.IO) { fetchFeed() }?.let { UpdateFeed.pick(it, installed) }
        withContext(Dispatchers.IO) { clean(installed) }
        if (found == null) {
            state =
                if (!asked) {
                    State.Idle
                } else if (lastFetchFailed) {
                    State.Failed
                } else {
                    State.Current
                }
            return
        }
        if (!UpdateRules.mayDownload(asked, metered(), playing())) {
            state = State.Idle
            return
        }
        state = State.Downloading(found.version, null)
        val file =
            withContext(Dispatchers.IO) {
                download(found) { percent ->
                    scope.launch { if (state is State.Downloading) state = State.Downloading(found.version, percent) }
                }?.takeIf { archiveOk(it, found) }
                    ?.takeIf { stage(it) }
            }
        if (file == null) {
            state = if (asked) State.Failed else State.Idle
            return
        }
        offer = found
        staged = file
        confirm = null
        state = State.Ready(found.version, silent())
    }

    private var lastFetchFailed = false

    /** The feed's body when it downloads and its signature holds, else null. */
    private fun fetchFeed(): JsonObject? {
        lastFetchFailed = true
        val text =
            try {
                get(feedUrl) { stream ->
                    val bytes = stream.readNBytesCompat(UpdateFeed.MAX_FEED_BYTES + 1)
                    if (bytes.size > UpdateFeed.MAX_FEED_BYTES) throw IOException("the feed is too large")
                    String(bytes, Charsets.UTF_8)
                }
            } catch (e: IOException) {
                CoreLog.w("update: the feed did not load: ${e.message}")
                return null
            }
        val body = UpdateFeed.verified(text, key)
        if (body == null) CoreLog.w("update: the feed's signature does not hold")
        lastFetchFailed = body == null
        return body
    }

    /** The APK in [dir], kept from an earlier run when its hash still holds; capped at the offer's size. */
    private fun download(
        offer: UpdateFeed.Offer,
        onPercent: (Int) -> Unit,
    ): File? {
        val file = File(dir, "${offer.versionCode}.apk")
        if (file.isFile && file.length() == offer.size && sha256(file) == offer.sha256) return file
        dir.mkdirs()
        val part = File(dir, "${offer.versionCode}.part")
        try {
            get(offer.url) { input ->
                part.outputStream().use { out ->
                    val buffer = ByteArray(BUFFER)
                    var total = 0L
                    var said = -1
                    while (true) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        total += n
                        if (total > offer.size) throw IOException("the APK is larger than the feed says")
                        out.write(buffer, 0, n)
                        val percent = (total * 100 / offer.size).toInt()
                        if (percent != said) {
                            said = percent
                            onPercent(percent)
                        }
                    }
                }
            }
        } catch (e: IOException) {
            CoreLog.w("update: the APK did not download: ${e.message}")
            part.delete()
            return null
        }
        if (part.length() != offer.size || sha256(part) != offer.sha256) {
            CoreLog.w("update: the APK's size or hash differs from the feed's")
            part.delete()
            return null
        }
        if (!part.renameTo(file)) return null
        return file
    }

    /** Our package, the offered versionCode and the running app's signers; Android checks the signer again. */
    private fun archiveOk(
        file: File,
        offer: UpdateFeed.Offer,
    ): Boolean {
        val archive = archiveInfo(file.path)
        val own = runCatching { packageInfo(pkg) }.getOrNull()
        val ok =
            archive != null &&
                own != null &&
                UpdateFeed.archiveMatches(
                    offer,
                    archive.packageName,
                    PackageInfoCompat.getLongVersionCode(archive),
                    signers(archive),
                    pkg,
                    signers(own),
                )
        if (!ok) {
            CoreLog.w("update: the APK's package, version or signer is not this app's; it is not installed")
            file.delete()
        }
        return ok
    }

    /** A session with the APK written and synced; only the commit waits. */
    private fun stage(file: File): Boolean =
        try {
            val installer = pm.packageInstaller
            val params =
                PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                    setAppPackageName(pkg)
                    setSize(file.length())
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
                    }
                }
            val id = installer.createSession(params)
            installer.openSession(id).use { s ->
                s.openWrite("base.apk", 0, file.length()).use { out ->
                    file.inputStream().use { it.copyTo(out) }
                    s.fsync(out)
                }
            }
            session = id
            true
        } catch (e: Exception) {
            CoreLog.w("update: the install could not be staged: ${e.message}")
            false
        }

    private fun commit(asked: Boolean = false) {
        if (committing) return
        val file = staged ?: return
        if (session == NO_SESSION && !stage(file)) return
        committing = true
        userAsked = asked
        try {
            val intent = Intent(app, UpdateReceiver::class.java).setPackage(pkg)
            val mutable = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
            val status = PendingIntent.getBroadcast(app, session, intent, PendingIntent.FLAG_UPDATE_CURRENT or mutable)
            pm.packageInstaller.openSession(session).use { it.commit(status.intentSender) }
        } catch (e: Exception) {
            CoreLog.w("update: the install could not start: ${e.message}")
            committing = false
        }
        // A session commits once; a failed or refused install stages again from the file.
        session = NO_SESSION
    }

    private var userAsked = false

    /** UpdateReceiver: the session's status. */
    internal fun onStatus(intent: Intent) {
        committing = false
        val version = offer?.version ?: return
        when (val code = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                // Never opened from the background: it waits for the user's Install (the Settings row, Home's line).
                confirm = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)
                state = State.Ready(version, silent = false)
                if (userAsked && visible) confirm?.let(::open)
            }

            PackageInstaller.STATUS_SUCCESS -> {
                CoreLog.w("update: $version installed")
            }

            else -> {
                val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
                CoreLog.w("update: the install of $version ended with status $code: $message")
                confirm = null
                state = State.Ready(version, silent = false)
            }
        }
    }

    private fun open(dialog: Intent) {
        runCatching { app.startActivity(dialog.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            .onFailure { CoreLog.w("update: the install dialog did not open: ${it.message}") }
    }

    private fun silent(): Boolean {
        val allowed = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && pm.canRequestPackageInstalls()
        val owner =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                runCatching { pm.getInstallSourceInfo(pkg).updateOwnerPackageName }.getOrNull()
            } else {
                null
            }
        return UpdateRules.silent(Build.VERSION.SDK_INT, allowed, owner, pkg)
    }

    private fun playing() = PlayerHost.live.get() > 0

    private fun downloading() = app.downloadKeeper.running

    private fun metered() = app.getSystemService(ConnectivityManager::class.java)?.isActiveNetworkMetered ?: true

    private fun installedCode() = PackageInfoCompat.getLongVersionCode(packageInfo(pkg, 0))

    /** APKs of a version already installed, or of another than [offer], go. */
    private fun clean(installed: Long) {
        dir.listFiles()?.forEach { f ->
            val code = f.nameWithoutExtension.toLongOrNull()
            if (code == null || code <= installed || f.extension == "part") f.delete()
        }
    }

    private fun packageInfo(
        name: String,
        flags: Int = SIGNERS,
    ): PackageInfo =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            pm.getPackageInfo(name, PackageManager.PackageInfoFlags.of(flags.toLong()))
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageInfo(name, flags)
        }

    private fun archiveInfo(path: String): PackageInfo? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            pm.getPackageArchiveInfo(path, PackageManager.PackageInfoFlags.of(SIGNERS.toLong()))
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageArchiveInfo(path, SIGNERS)
        }

    /** Each signing certificate's SHA-256, as hex. */
    private fun signers(info: PackageInfo): Set<String> {
        val certs =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                info.signingInfo?.apkContentsSigners
            } else {
                @Suppress("DEPRECATION")
                info.signatures
            }
        return certs.orEmpty().map { hex(MessageDigest.getInstance("SHA-256").digest(it.toByteArray())) }.toSet()
    }

    private companion object {
        const val NO_SESSION = -1
        const val BUFFER = 64 * 1024
        const val DAY_MS = 24L * 60 * 60 * 1000
        const val SPREAD_MS = 2L * 60 * 60 * 1000
        const val TIMEOUT_MS = 30_000

        @Suppress("DEPRECATION")
        val SIGNERS =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                PackageManager.GET_SIGNING_CERTIFICATES
            } else {
                PackageManager.GET_SIGNATURES
            }

        /** One GET over HTTPS (redirects followed, as GitHub's 302 to its asset host); a non-200 is an IOException. */
        fun <T> get(
            url: String,
            read: (java.io.InputStream) -> T,
        ): T {
            val connection = URL(url).openConnection() as HttpURLConnection
            try {
                connection.connectTimeout = TIMEOUT_MS
                connection.readTimeout = TIMEOUT_MS
                connection.instanceFollowRedirects = true
                val code = connection.responseCode
                if (code != HttpURLConnection.HTTP_OK) throw IOException("HTTP $code")
                return connection.inputStream.use(read)
            } finally {
                connection.disconnect()
            }
        }

        fun sha256(file: File): String {
            val digest = MessageDigest.getInstance("SHA-256")
            file.inputStream().use { input ->
                val buffer = ByteArray(BUFFER)
                while (true) {
                    val n = input.read(buffer)
                    if (n < 0) break
                    digest.update(buffer, 0, n)
                }
            }
            return hex(digest.digest())
        }

        fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it) }

        /** InputStream.readNBytes is API 33. */
        fun java.io.InputStream.readNBytesCompat(limit: Int): ByteArray {
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(8 * 1024)
            while (out.size() < limit) {
                val n = read(buffer, 0, minOf(buffer.size, limit - out.size()))
                if (n < 0) break
                out.write(buffer, 0, n)
            }
            return out.toByteArray()
        }
    }
}

/** The install session's status, handed to the app's [Updater]. */
class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        (context.applicationContext as MarqueeApp).updater?.onStatus(intent)
    }
}
