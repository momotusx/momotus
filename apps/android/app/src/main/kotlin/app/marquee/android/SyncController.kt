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
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The sync account on this device, for the app's life (plans/android-design-v2.md 13c, Apple's
 * `SyncModel`). The keystore record lives in the [CoreSession.secrets]; every round gets it and may
 * hand back a new one, which is stored before anything else happens.
 *
 * Rounds run in one worker owned by the app, never by a screen or a timer, so leaving the app or a
 * screen cannot cancel a round between the core's answer and storing its record. Asking for a round
 * while one runs keeps one more for after it. Rounds are asked for once the core runs, when the app
 * comes to the front and when it goes to the back (so edits leave with the user), every [EVERY_MS]
 * while it is in front, and by Sync now.
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

    private val wake = Channel<Unit>(Channel.CONFLATED)
    private var timer: Job? = null

    /** Bumped by every join, under this object's lock: a round from before it stores nothing. */
    private var account = 0L

    /** The worker, once the core runs, and the first round. */
    fun start() {
        session.scope.launch {
            snapshotFlow { session.health }.first { it == CoreHealth.Running }
            for (ask in wake) rounds()
        }
        request()
    }

    /** Asks for a round: now, or after the one running. */
    fun request() {
        wake.trySend(Unit)
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

    /**
     * After pairing: the record replaces any account before it, then the first round. False when the
     * keystore could keep it for this session only; sync then waits until it is stored.
     */
    suspend fun didJoin(record: SyncRecord): Boolean {
        // The keystore write syncs to disk: never on the main thread.
        val durable =
            withContext(Dispatchers.IO) {
                synchronized(this@SyncController) {
                    account++
                    session.secrets.update { it.copy(syncRecord = record) }
                    durable()
                }
            }
        joined = true
        problem = if (durable) null else SyncCopy.JOINED_SESSION_ONLY
        request()
        return durable
    }

    private fun durable() = session.secrets.health != SecretStore.Health.MemoryOnly

    /**
     * One sync, storing each new record and going again while the core asks to (a round that changed
     * the record publishes nothing until it is stored), at most [ROUNDS] times. Nothing is sent while
     * the record exists in memory only: the next round stores it again first.
     */
    private suspend fun rounds() {
        if (session.health != CoreHealth.Running) return
        val (mine, stored) = synchronized(this) { account to session.secrets.load().syncRecord }
        var record = stored ?: return
        if (!durable()) {
            session.secrets.update { it }
            if (!durable()) {
                problem = SyncCopy.STORAGE
                return
            }
        }
        syncing = true
        try {
            repeat(ROUNDS) {
                val report =
                    try {
                        session.ops.syncNow(record)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        // The words only: the raw error may name the relay's address.
                        if (synchronized(this) { account == mine }) {
                            problem = SyncCopy.problem((e as? CoreError)?.code, e.message.orEmpty())
                        }
                        CoreLog.w("sync did not finish: ${(e as? CoreError)?.code ?: e.javaClass.simpleName}")
                        return
                    }
                val kept =
                    synchronized(this) {
                        // A join replaced the account while this round ran: its answer is not ours.
                        if (account != mine) return
                        report.record?.let { next ->
                            session.secrets.update { it.copy(syncRecord = next) }
                            record = next
                        }
                        durable()
                    }
                CoreLog.d(
                    "sync round: pushed ${report.pushed}, pulled ${report.pulled}, stored ${report.record != null}",
                )
                lastReport = report
                lastSynced = System.currentTimeMillis()
                if (!kept) {
                    problem = SyncCopy.STORAGE
                    return
                }
                problem = null
                if (!report.again) return
            }
        } finally {
            syncing = false
        }
    }

    private companion object {
        const val ROUNDS = 5
        const val EVERY_MS = 5 * 60_000L
    }
}
