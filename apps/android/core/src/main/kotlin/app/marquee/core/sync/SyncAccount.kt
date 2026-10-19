package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.SyncCopy
import app.marquee.core.SyncRules
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Devices
import app.marquee.core.model.LostPreview
import app.marquee.core.model.SyncRecord
import app.marquee.core.model.SyncReport
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/** Where the account's record lives. It holds a token and keys: never logged or shown. */
interface RecordStore {
    fun load(): SyncRecord?

    /** Stores [record], or deletes it with null; false when it is held in memory only (nothing is sent then). */
    fun save(record: SyncRecord?): Boolean
}

/** The record in the app's encrypted secret store (Android Keystore key, no backup). */
class SecretRecordStore(
    private val secrets: SecretStore,
) : RecordStore {
    override fun load() = secrets.load().syncRecord

    override fun save(record: SyncRecord?): Boolean {
        secrets.update { it.copy(syncRecord = record) }
        return secrets.health != SecretStore.Health.MemoryOnly
    }
}

/**
 * The sync account on this device, one per app (Apple's `SyncModel`, the web's `core/sync.ts`).
 *
 * Every op on the record runs in turn (one at a time, in the order asked) in [scope], the app's, never
 * a page's: leaving a page cannot cancel an op between the core's answer and storing its record. A
 * record that comes back is kept only if it is not older than the one stored (`revision`). Rounds run
 * in one worker: asking while a round runs keeps one more for after it. `dropped` forgets the account
 * here; `fork` stays a problem until the user takes the server's list.
 */
open class SyncAccount(
    private val ops: CoreOps,
    private val store: RecordStore,
    private val scope: CoroutineScope,
    /** Waits until the core runs: a round asked for while it restarts waits rather than fails. */
    private val running: suspend () -> Unit = {},
    private val io: CoroutineDispatcher = Dispatchers.IO,
    recoveredAtStart: Boolean = false,
    /** Keeps [recovered] across launches. */
    private val saveRecovered: (Boolean) -> Unit = {},
) {
    /** A failure in words, with the core's code when it has one (the page offers a fix for some). */
    data class Problem(
        val code: String?,
        val words: String,
    )

    /** A record is stored, or held for this session: this device is in a sync account. */
    var joined by mutableStateOf(store.load() != null)
        private set
    var syncing by mutableStateOf(false)
        private set
    var lastReport: SyncReport? by mutableStateOf(null)
        private set

    /** Wall clock of the last good round. */
    var lastSynced: Long? by mutableStateOf(null)
        private set

    /** The device list as last fetched; its 12 characters are the ones to compare. */
    var devices: Devices? by mutableStateOf(null)
        private set

    /** What went wrong in the last round, in words. */
    var problem: Problem? by mutableStateOf(null)
        private set

    /** A join is being stored: the pairing page holds Done until it is. */
    var joining by mutableStateOf(false)
        private set

    /** Joined with a recovery kit and no new kit made since: the page asks for one. */
    var recovered by mutableStateOf(recoveredAtStart)
        private set

    private val wake = Channel<Unit>(Channel.CONFLATED)

    /** One op on the record at a time. */
    private val line = Mutex()

    /** Bumped by every join and leave, under [storeLock]: an op from before it stores and shows nothing. */
    @Volatile
    private var account = 0L

    /** Guards the check of [account] and the store write together, so a leave never lets an old op's record back. */
    private val storeLock = Any()

    /** The keystore holds the record in memory only: it is stored again before anything is sent. */
    @Volatile
    private var heldOnly = false

    /** The worker. Call once. */
    fun startRounds() {
        scope.launch { for (ask in wake) rounds() }
    }

    /** Asks for a round: now, or after the one running. */
    fun request() {
        wake.trySend(Unit)
    }

    /**
     * One sync, going again while the core asks to (a round that changed the record publishes nothing
     * until it is stored).
     */
    private suspend fun rounds() {
        running()
        if (!joined) return
        syncing = true
        try {
            repeat(ROUNDS) {
                if (!joined) return
                val mine = account
                val report =
                    try {
                        withRecord("syncNow", { ops.syncNow(it) }) { it.record }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        if (account == mine) problem = problemOf(e, SyncCopy.Action.Round)
                        CoreLog.w("sync did not finish: ${(e as? CoreError)?.code ?: e.javaClass.simpleName}")
                        return
                    }
                // A join or a leave replaced the account while this round ran: its answer is not ours.
                if (account != mine) return
                CoreLog.d(
                    "sync round: pushed ${report.pushed}, pulled ${report.pulled}, stored ${report.record != null}",
                )
                lastReport = report
                lastSynced = System.currentTimeMillis()
                problem = null
                // The 12 characters to compare come from the device list: keep it as fresh as the round.
                if (report.fingerprint != devices?.fingerprint) runCatching { loadDevices() }
                if (!report.again) return
            }
        } finally {
            syncing = false
        }
    }

    /**
     * A new account's record (paired, started or recovered) replaces any before it, then the first
     * round runs. When the keystore keeps it for this session only, sync waits until it is stored. The
     * work is the app's, not the page's: leaving the page at once never loses or half-stores a join.
     */
    fun join(
        record: SyncRecord,
        recoveredWithKit: Boolean = false,
    ): Job {
        joining = true
        synchronized(storeLock) { account++ }
        return scope.launch {
            // A new account's record is stored as it is: revisions compare only within one account.
            val durable = withContext(io) { synchronized(storeLock) { store.save(record) } }
            heldOnly = !durable
            joined = true
            recovered = recoveredWithKit
            saveRecovered(recoveredWithKit)
            lastReport = null
            lastSynced = null
            devices = null
            problem = if (durable) null else Problem("storage", SyncCopy.JOINED_SESSION_ONLY)
            joining = false
            request()
        }
    }

    /** After pairing: the join goes to the app, which stores it whatever the page does next. */
    fun didJoin(record: SyncRecord) {
        join(record)
    }

    /** The first device of a new account. Returns once the record is stored. */
    suspend fun createAccount(
        server: String,
        invite: String,
        name: String,
    ) {
        val record = ops.createSyncAccount(server, name, invite.ifEmpty { null })
        join(record).join()
    }

    /** A new device joining with a recovery kit; the page then asks for a new kit. */
    suspend fun recover(
        server: String,
        kit: String,
        name: String,
    ) {
        val record = ops.recover(server, kit, name)
        join(record, recoveredWithKit = true).join()
    }

    /** The device list, fresh from the sync server. */
    suspend fun loadDevices(): Devices {
        val mine = account
        val list = withRecord("syncDevices", { ops.syncDevices(it) }) { it.record }
        if (account == mine) devices = list
        return list
    }

    /** Vouches for the device that shows [qr]; returns the six digits it must show too. */
    suspend fun approvePairing(qr: String): String {
        val approved = withRecord("approvePairing", { ops.approvePairing(it, qr) }) { it.record }
        // The new device joins once it confirms; the list catches up on the next look.
        runCatching { loadDevices() }
        return approved.code
    }

    /** A recovery kit, to show once: nothing here keeps it. The page asks who is there first. */
    suspend fun createRecoveryKit(): String {
        val made = withRecord("createRecoveryKit", { ops.createRecoveryKit(it) }) { it.record }
        recovered = false
        saveRecovered(false)
        runCatching { loadDevices() }
        return made.kit
    }

    /** What reporting [lost] would drop: its devices (ticked) and its kits (always). */
    suspend fun lostPreview(lost: String): LostPreview =
        withRecord("lostPreview", { ops.lostPreview(it, lost) }) { null }

    /**
     * Removes a device or a kit. It has landed once this returns: a list that cannot be fetched right
     * after does not undo it.
     */
    suspend fun removeDevice(device: String) {
        withRecord("removeDevice", { ops.removeDevice(it, device) }) { it }
        runCatching { loadDevices() }
    }

    /** Drops [lost] and the devices it added except [keep]; their kits go too. */
    suspend fun reportLost(
        lost: String,
        keep: List<String>,
    ) {
        withRecord("reportLost", { ops.reportLost(it, lost, keep) }) { it }
        runCatching { loadDevices() }
    }

    /** Leaves the account; the library stays. Without [force] the core refuses while edits are unsent (`unsent`). */
    suspend fun signOut(force: Boolean) {
        withRecord("signOut", { ops.signOut(it, force) }) { null }
        forget()
        problem = null
    }

    /** After `fork`: the sync server's device list wins over this device's. */
    suspend fun useServerRoster() {
        withRecord("useServerRoster", { ops.useServerRoster(it) }) { it }
        problem = null
        request()
    }

    /**
     * Drops the account here only, when the relay cannot take this device back (wrong relay, entries
     * lost). The library stays.
     */
    fun forgetHere() {
        scope.launch {
            forget()
            problem = null
        }
    }

    /**
     * Runs one op on the record, in turn, in the app's scope (the caller may go away; the op does
     * not). A record in the answer is stored before the next op runs.
     */
    private suspend fun <T> withRecord(
        name: String,
        call: suspend (SyncRecord) -> T,
        recordOf: (T) -> SyncRecord?,
    ): T =
        scope
            .async {
                line.withLock {
                    val mine = account
                    val record =
                        storedRecord() ?: throw CoreError("this device has not joined the account", code = "notJoined")
                    try {
                        val result = call(record)
                        val next = recordOf(result)
                        if (next != null && !keep(next, mine)) {
                            throw CoreError("the record was not stored", code = "storage")
                        }
                        result
                    } catch (e: CoreError) {
                        if (account == mine && (e.code == "dropped" || e.code == "fork")) {
                            CoreLog.w("sync $name: ${e.code}")
                            if (e.code == "dropped") forget()
                            problem = problemOf(e, SyncCopy.Action.Round)
                        }
                        throw e
                    }
                }
            }.await()

    /**
     * The stored record; one the keystore held in memory only is stored again first, and nothing is
     * sent while it is not.
     */
    private suspend fun storedRecord(): SyncRecord? {
        val record = withContext(io) { store.load() } ?: return null
        if (heldOnly) {
            heldOnly = !withContext(io) { synchronized(storeLock) { store.save(record) } }
            if (heldOnly) {
                problem = Problem("storage", SyncCopy.STORAGE)
                throw CoreError("the record was not stored", code = "storage")
            }
        }
        return record
    }

    /** Stores a record the core handed back unless the stored one is newer, and only for the account that asked. */
    private suspend fun keep(
        next: SyncRecord,
        mine: Long,
    ): Boolean {
        val durable =
            withContext(io) {
                synchronized(storeLock) {
                    if (account != mine) null else store.save(SyncRules.newer(store.load(), next))
                }
            } ?: return true
        heldOnly = !durable
        if (!durable) problem = Problem("storage", SyncCopy.STORAGE)
        return durable
    }

    private suspend fun forget() {
        withContext(io) {
            synchronized(storeLock) {
                account++
                store.save(null)
            }
        }
        heldOnly = false
        joined = false
        devices = null
        lastReport = null
        lastSynced = null
        recovered = false
        saveRecovered(false)
    }

    companion object {
        private const val ROUNDS = 5

        /** A failure of [action] in words, with its code. */
        fun problemOf(
            e: Throwable,
            action: SyncCopy.Action,
        ): Problem {
            val code = (e as? CoreError)?.code
            return Problem(code, SyncCopy.problem(code, (e as? CoreError)?.message ?: e.message.orEmpty(), action))
        }
    }
}
