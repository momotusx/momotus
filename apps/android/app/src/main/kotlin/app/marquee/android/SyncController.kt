package app.marquee.android

import android.content.Context
import androidx.compose.runtime.snapshotFlow
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.core.sync.SecretRecordStore
import app.marquee.core.sync.SyncAccount
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * The sync account on this device, for the app's life (plans/android-design-v2.md 13c): the core's
 * [SyncAccount] with the record in the encrypted secret store, and the rounds the app's life asks for:
 * once the core runs (at start and after every restart), when the app comes to the front and when it
 * goes to the back (so edits leave with the user), every [EVERY_MS] while it is in front, and by Sync
 * now.
 */
class SyncController(
    private val session: CoreSession,
    context: Context,
) : SyncAccount(
        session.ops,
        SecretRecordStore(session.secrets),
        session.scope,
        running = { snapshotFlow { session.health }.first { it == CoreHealth.Running } },
        recoveredAtStart = context.prefs().getBoolean(RECOVERED, false),
        saveRecovered = {
            context
                .prefs()
                .edit()
                .putBoolean(RECOVERED, it)
                .apply()
        },
        pendingKitsAtStart =
            context
                .prefs()
                .getString(PENDING_KITS, null)
                ?.split(',')
                ?.filter { it.isNotEmpty() }
                .orEmpty(),
        savePendingKits = {
            context
                .prefs()
                .edit()
                .putString(PENDING_KITS, it.joinToString(","))
                .apply()
        },
    ) {
    private var timer: Job? = null

    /** The worker, and a round each time the core runs. */
    fun start() {
        startRounds()
        session.scope.launch {
            snapshotFlow { session.health }.filter { it == CoreHealth.Running }.collect { request() }
        }
    }

    /** In front: a round now, then one every [EVERY_MS]. The timer only asks; it owns no round. */
    fun foreground() {
        timer?.cancel()
        timer =
            session.scope.launch {
                while (isActive) {
                    request()
                    delay(EVERY_MS)
                }
            }
    }

    /** To the back: the timer stops, and what is queued goes out. */
    fun background() {
        timer?.cancel()
        timer = null
        request()
    }

    private companion object {
        const val EVERY_MS = 5 * 60_000L
        const val RECOVERED = "syncRecovered"
        const val PENDING_KITS = "syncPendingKits"

        fun Context.prefs() = getSharedPreferences("marquee", Context.MODE_PRIVATE)
    }
}
