package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import app.marquee.core.SyncCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.SyncRecord
import app.marquee.core.model.SyncReport
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex

/**
 * The sync account on this device, for the app's life (plans/android-design-v2.md 13c, Apple's
 * `SyncModel`). The keystore record lives in the [CoreSession.secrets]; every round gets it and may
 * hand back a new one, which is stored before anything else happens. Rounds run once the core
 * runs, when the app comes to the front and when it goes to the back (so edits leave with the
 * user), and every [EVERY_MS] while it is in front.
 */
class SyncController(
    private val session: CoreSession,
) {
    /** A record is stored: this device is in a sync account. */
    var joined by mutableStateOf(session.secrets.load().syncRecord != null)
        private set
    var syncing by mutableStateOf(false)
        private set
    var lastReport: SyncReport? by mutableStateOf(null)
        private set

    /** Wall clock of the last good round. */
    var lastSynced: Long? by mutableStateOf(null)
        private set

    /** What went wrong, in words. */
    var problem: String? by mutableStateOf(null)
        private set

    private val one = Mutex()
    private var timer: Job? = null

    /** The first round once the core runs. */
    fun start() {
        session.scope.launch {
            snapshotFlow { session.health }.first { it == CoreHealth.Running }
            syncNow()
        }
    }

    /** In front: a round now, then one every [EVERY_MS]. */
    fun foreground() {
        timer?.cancel()
        timer =
            session.scope.launch {
                while (isActive) {
                    syncNow()
                    delay(EVERY_MS)
                }
            }
    }

    /** To the back: the timer stops, and what is queued goes out. */
    fun background() {
        timer?.cancel()
        timer = null
        session.scope.launch { syncNow() }
    }

    /** After pairing: store the record, then sync. */
    fun didJoin(record: SyncRecord) {
        session.secrets.update { it.copy(syncRecord = record) }
        joined = true
        session.scope.launch { syncNow() }
    }

    /**
     * One sync, storing each new record and going again while the core asks to (a round that changed
     * the record publishes nothing until it is stored), at most [ROUNDS] times. A sync already
     * running makes this one return at once.
     */
    suspend fun syncNow() {
        if (session.health != CoreHealth.Running || !one.tryLock()) return
        try {
            var record = session.secrets.load().syncRecord ?: return
            syncing = true
            repeat(ROUNDS) {
                val report =
                    try {
                        session.ops.syncNow(record)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        // The words only: the raw error may name the relay's address.
                        problem = SyncCopy.problem((e as? CoreError)?.code, e.message.orEmpty())
                        CoreLog.w("sync did not finish: ${(e as? CoreError)?.code ?: e.javaClass.simpleName}")
                        return
                    }
                report.record?.let { next ->
                    session.secrets.update { it.copy(syncRecord = next) }
                    record = next
                }
                lastReport = report
                lastSynced = System.currentTimeMillis()
                problem = null
                if (!report.again) return
            }
        } finally {
            syncing = false
            one.unlock()
        }
    }

    private companion object {
        const val ROUNDS = 5
        const val EVERY_MS = 5 * 60_000L
    }
}
