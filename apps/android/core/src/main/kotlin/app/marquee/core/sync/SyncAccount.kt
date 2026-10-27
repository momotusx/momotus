package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.SyncCopy
import app.marquee.core.SyncRules
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Announcement
import app.marquee.core.model.Devices
import app.marquee.core.model.ForkPreview
import app.marquee.core.model.IdentityLink
import app.marquee.core.model.LostPreview
import app.marquee.core.model.ScheduledDeletion
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
import kotlinx.coroutines.delay
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
 * in one worker: asking while a round runs keeps one more for after it. `dropped` and `accountGone`
 * forget the account here; `fork` stays a problem until the user takes the server's list.
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
    /** The kits made in earlier runs whose "I saved it" never came: their ids. */
    pendingKitsAtStart: List<String> = emptyList(),
    /** Keeps those ids across launches (never a kit itself). */
    private val savePendingKits: (List<String>) -> Unit = {},
    /** How long a made kit's letters stay in memory once its page is left. */
    private val holdMs: Long = KIT_HOLD_MS,
    /** Another device deleted the account and nobody pressed OK yet (an earlier run heard it). */
    goneAtStart: Boolean = false,
    /** Keeps that across launches until OK or a join. */
    private val saveGone: (Boolean) -> Unit = {},
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

    /** The device list as last fetched. */
    var devices: Devices? by mutableStateOf(null)
        private set

    /** The 12 characters to compare, from whichever answer came last: a round or the device list. */
    var fingerprint: String? by mutableStateOf(null)
        private set

    /**
     * The last read of the device list failed: the list, and the 12 characters when they came with it,
     * may be out of date until a read works again.
     */
    var listFailed by mutableStateOf(false)
        private set

    /** Changes that failed since this device joined, kept until the user dismisses them. */
    var failed by mutableStateOf(Failed())
        private set

    data class Failed(
        val refused: Int = 0,
        val unreadable: Int = 0,
        val newer: Int = 0,
    )

    /** What went wrong in the last round, in words. */
    var problem: Problem? by mutableStateOf(if (goneAtStart) Problem(GONE, SyncCopy.ACCOUNT_GONE) else null)
        private set

    /** The last round ended where the server will not let this device leave: only stopping here helps. */
    val stuck: Boolean get() = stuck(problem?.code)

    /** A join is being stored: the pairing page holds Done until it is. */
    var joining by mutableStateOf(false)
        private set

    /** Joined with a recovery kit and no new kit made since: the page asks for one. */
    var recovered by mutableStateOf(recoveredAtStart)
        private set

    /** The last leave was here only: the sync server still lists this device. */
    var leftHere by mutableStateOf(false)
        private set

    /** This device deleted the account: the page says so, as after a sign-out. */
    var deletedHere by mutableStateOf(false)
        private set

    /** A sign-in another account holds, met after this device joined another way: its provider and ticket. */
    data class HeldLink(
        val provider: String,
        val ticket: String,
    ) {
        override fun toString() = "HeldLink(provider=$provider, ticket=hidden)"
    }

    /** Waits, in memory only, for Account's "Use it for this account instead" (the ticket's 10 minutes). */
    var heldLink: HeldLink? by mutableStateOf(null)

    /**
     * The device that approved this one's pairing (plans/tv-qr-login.md 5.5): Home says "Signed in from" it for a
     * moment. Memory only.
     */
    var joinedBy: String? by mutableStateOf(null)

    /** A kit made and not yet confirmed saved: its id (its row in the device list) and its letters. */
    data class HeldKit(
        val id: String,
        val kit: String,
    ) {
        override fun toString() = "HeldKit(id=$id)"
    }

    /**
     * The kit made and not yet confirmed saved, in memory only (never on disk): the kit page shows it again
     * after the screen lock, until the user says it is saved. Once its page is left it stays [holdMs] at
     * most; it goes when its row leaves the list.
     */
    var heldKit: HeldKit? by mutableStateOf(null)
        private set

    /**
     * Every kit made and not yet said saved, held or not, by id: a list, so a new kit never hides an older
     * one that went unseen.
     */
    private var pending by mutableStateOf(pendingKitsAtStart)

    /** Kits whose letters went before the user said they were saved: nobody holds them, the next kit removes them. */
    val unseenKits: List<String>
        get() = pending.toMutableList().apply { heldKit?.let { remove(it.id) } }

    /** The last kit to go unseen did so because its hold ran out (not because the app closed). */
    var kitExpired by mutableStateOf(false)
        private set

    private var holdTimer: Job? = null

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
                // The round says how things are now: a cancel made here is in it.
                deletionCancelled = false
                lastSynced = System.currentTimeMillis()
                fingerprint = report.fingerprint
                problem = null
                if (report.refused > 0 || report.unreadable > 0 || report.newer > 0) {
                    failed =
                        Failed(
                            failed.refused + report.refused,
                            failed.unreadable + report.unreadable,
                            failed.newer + report.newer,
                        )
                }
                // The list follows the round; when it cannot be read, the page says it may be out of date.
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
        // The bump and the write together, before join returns: an op from before stores nothing, and one
        // asked after runs on this account, never on the old record. A new account's record is stored as it
        // is (revisions compare only within one account). ponytail: one small write on the caller's thread.
        val durable =
            synchronized(storeLock) {
                account++
                store.save(record)
            }
        dropKits()
        leftHere = false
        deletedHere = false
        saveGone(false)
        return scope.launch {
            heldOnly = !durable
            joined = true
            recovered = recoveredWithKit
            saveRecovered(recoveredWithKit)
            lastReport = null
            lastSynced = null
            devices = null
            fingerprint = null
            listFailed = false
            failed = Failed()
            acked = emptySet()
            deletionCancelled = false
            problem = if (durable) null else Problem("storage", SyncCopy.JOINED_SESSION_ONLY)
            joining = false
            request()
        }
    }

    /** After pairing: the join goes to the app, which stores it whatever the page does next. */
    fun didJoin(record: SyncRecord) {
        // Saving shows at once, so Done never flickers; the keystore write happens off the main thread.
        joining = true
        scope.launch(io) { join(record) }
    }

    /** Forgets the failures listed since joining. */
    fun dismissFailed() {
        failed = Failed()
    }

    /**
     * The first device of a new account. Runs in the app's scope: a page left once the sync server has
     * answered still stores the record. Returns once it is stored.
     */
    suspend fun createAccount(
        server: String,
        invite: String,
        name: String,
        ticket: String? = null,
        move: Boolean = false,
        form: String? = null,
    ) = scope
        .async {
            val record = ops.createSyncAccount(server, name, invite.ifEmpty { null }, ticket, move, form)
            join(record).join()
        }.await()

    /**
     * A new device joining with a recovery kit or a passkey's PRF output, in the app's scope like
     * [createAccount]. Returns the way in (`kit` or `passkey`); only after a kit does the page ask for a new one.
     */
    suspend fun recover(
        server: String,
        kit: String?,
        name: String,
        prf: String? = null,
        form: String? = null,
    ): String =
        scope
            .async {
                val recovered = ops.recover(server, kit, name, prf, form)
                join(recovered.record, recoveredWithKit = recovered.via == "kit").join()
                recovered.via
            }.await()

    /** The account's sign-in links (any profile). */
    suspend fun identities(): List<IdentityLink> = withRecord("identities", { ops.identities(it) }) { it.record }.links

    /** Links a sign-in's identity here; [move] takes one linked elsewhere (the owner profile only, D16). */
    suspend fun linkIdentity(
        ticket: String,
        move: Boolean = false,
    ) {
        withRecord("linkIdentity", { ops.linkIdentity(it, ticket, move) }) { it.record }
    }

    /** Unlinks by link id, with Apple's revoke [receipt] for an Apple link when there is one (the owner, D16). */
    suspend fun unlinkIdentity(
        link: Long,
        receipt: String? = null,
    ) {
        withRecord("unlinkIdentity", { ops.unlinkIdentity(it, link, receipt) }) { it.record }
    }

    /** A passkey kit from the passkey's PRF output (the owner profile only, D17); returns its id. */
    suspend fun savePasskey(
        prf: String,
        label: String,
    ): String {
        val saved = withRecord("savePasskey", { ops.savePasskey(it, prf, label) }) { it.record }
        runCatching { loadDevices() }
        return saved.id
    }

    /** Cancels the scheduled deletion (any profile); [unlink] also removes the identity that asked for it. */
    suspend fun cancelDeletion(unlink: Boolean = false) {
        withRecord("cancelDeletion", { ops.cancelDeletion(it, unlink) }) { it.record }
        deletionCancelled = true
        request()
    }

    /** The account is scheduled for deletion, as the last round said, unless this device just cancelled it. */
    val deletion: ScheduledDeletion? get() = lastReport?.deletion?.takeUnless { deletionCancelled }

    private var deletionCancelled by mutableStateOf(false)

    /** Announcements the user dismissed on this run: gone at once, before the next round stops reporting them. */
    private var acked by mutableStateOf(emptySet<String>())

    /** The announcements the last round reported and nobody dismissed here yet. */
    val announcements: List<Announcement> get() = lastReport?.announcements.orEmpty().filter { it.id !in acked }

    /** The user dismissed [ids]; security notices are the owner's (`notOwner` from another profile, nothing hidden). */
    suspend fun ackAnnouncements(ids: List<String>) {
        ops.ackAnnouncements(ids)
        acked = acked + ids
    }

    /** The device list, fresh from the sync server. */
    suspend fun loadDevices(): Devices {
        val mine = account
        val list =
            try {
                withRecord("syncDevices", { ops.syncDevices(it) }) { it.record }
            } catch (e: Exception) {
                if (e !is CancellationException && account == mine) listFailed = true
                throw e
            }
        if (account == mine) {
            devices = list
            fingerprint = list.fingerprint
            listFailed = false
            followKits(list)
        }
        return list
    }

    /**
     * Vouches for the device that shows [qr] (plans/tv-qr-login.md Q2); returns its member id, which the
     * device list shows once it joined. [scanned]: this app's camera read it; [unlocked]: the screen lock
     * was just passed.
     */
    suspend fun approvePairing(
        qr: String,
        scanned: Boolean,
        unlocked: Boolean,
    ): String {
        val approved =
            withRecord("approvePairing", { ops.approvePairing(it, qr, scanned, unlocked) }) { it.record }
        return approved.member
    }

    /**
     * A recovery kit, held in memory until the user says it is saved, by the id the core answers with it.
     * A kit lost unseen goes first when the list shows it, and is never claimed gone while the list cannot
     * be read. All of it runs in the app's scope: a page left meanwhile still keeps the new kit and its id.
     */
    suspend fun createRecoveryKit(): String =
        scope
            .async {
                val before = runCatching { loadDevices() }.getOrNull()
                val kitsBefore =
                    before
                        ?.members
                        ?.filter { it.kit }
                        ?.map { it.id }
                        ?.toSet()
                for (id in unseenKits) {
                    when {
                        // The list could not say: the kit stays pending, never claimed gone.
                        kitsBefore == null -> {
                            Unit
                        }

                        id in kitsBefore -> {
                            withRecord("removeDevice", { ops.removeDevice(it, id) }) { it }
                            settle(id)
                        }

                        else -> {
                            settle(id)
                        }
                    }
                }
                val made = withRecord("createRecoveryKit", { ops.createRecoveryKit(it) }) { it.record }
                recovered = false
                saveRecovered(false)
                // The list shows the new row; its id came with the kit.
                runCatching { loadDevices() }
                hold(HeldKit(made.id, made.kit))
                made.kit
            }.await()

    /** The user saved the kit: it leaves memory, and nothing marks it pending. */
    fun kitSaved() {
        holdTimer?.cancel()
        heldKit?.let { kit -> pending = pending.toMutableList().apply { remove(kit.id) } }
        heldKit = null
        savePendingKits(pending)
    }

    /**
     * The kit page is open, or was left: the hold never runs out while the kit can be on screen, and starts
     * once the page is left.
     */
    fun kitPage(open: Boolean) {
        holdTimer?.cancel()
        val kit = heldKit ?: return
        if (open) return
        // ponytail: a fixed hold after the page; the letters never outlive it, and the kit then counts as unseen.
        holdTimer =
            scope.launch {
                delay(holdMs)
                if (heldKit === kit) {
                    heldKit = null
                    kitExpired = true
                }
            }
    }

    private fun hold(kit: HeldKit) {
        holdTimer?.cancel()
        heldKit = kit
        kitExpired = false
        pending = pending + kit.id
        savePendingKits(pending)
    }

    /** A pending kit is settled (removed, or gone from the list): it leaves the list, and memory if held. */
    private fun settle(id: String) {
        if (heldKit?.id == id) {
            holdTimer?.cancel()
            heldKit = null
        }
        pending = pending.toMutableList().apply { remove(id) }
        savePendingKits(pending)
    }

    /** A held or unseen kit whose row left the list is gone: nobody needs to save or remove it. */
    private fun followKits(list: Devices) {
        val ids = list.members.map { it.id }.toSet()
        for (id in pending.filter { it !in ids }) settle(id)
    }

    private fun dropKits() {
        holdTimer?.cancel()
        heldKit = null
        kitExpired = false
        pending = emptyList()
        savePendingKits(pending)
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
        // A held or unseen kit that was just removed needs no saving and no removing, list or no list.
        if (device in pending) settle(device)
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
    suspend fun signOut(
        force: Boolean,
        removeAfter: Boolean? = null,
    ) = scope
        .async {
            // All of it in the app's scope: Back while the core answers still forgets the account here. With
            // [removeAfter] the core records D13's removal (and whether debrid goes) before the leave goes out.
            withRecord("signOut", { ops.signOut(it, force, removeAfter = removeAfter) }) { null }
            forget()
            problem = null
            leftHere = false
            deletedHere = false
        }.await()

    /**
     * Deletes the account on the sync server for every device. Once the server confirmed, this device
     * forgets it as after [signOut] (the library stays) and the page says the account was deleted, as it
     * says a sign-out (the OK line is for news from another device).
     */
    suspend fun deleteAccount(receipt: String? = null) =
        scope
            .async {
                // All of it in the app's scope: Back while the core answers still forgets the account here.
                withRecord("deleteAccount", { ops.deleteAccount(it, receipt) }) { null }
                forget()
                problem = null
                leftHere = false
                deletedHere = true
            }.await()

    /** OK on "Your Marquee account was deleted": the line goes, also from the next launch. */
    fun dismissGone() {
        if (problem?.code == GONE) problem = null
        saveGone(false)
    }

    /** After `fork`: both device lists, read only, for the user to compare. */
    suspend fun forkPreview(): ForkPreview = withRecord("forkPreview", { ops.forkPreview(it) }) { null }

    /**
     * After `fork`: the sync server's device list as [forkPreview] showed it ([head]), without [drop]. A
     * list that moved since is refused (`rosterChanged`): compare again.
     */
    suspend fun useServerRoster(
        drop: List<String>,
        head: String,
    ) {
        withRecord("useServerRoster", { ops.useServerRoster(it, drop, head) }) { it }
        problem = null
        request()
    }

    /**
     * Stop syncing here only: no round and nothing sent; the core forgets the account's keys, roster and
     * cursors and this device its record, so it can pair again. For a device the sync server will not let
     * leave (off its list, a lost token, a fork it cannot settle). The library stays.
     */
    suspend fun leaveHere() =
        scope
            .async {
                // All of it in the app's scope: Back while the core forgets the account still forgets it here.
                withRecord("signOut", { ops.signOut(it, force = false, local = true) }) { null }
                forget()
                problem = null
                leftHere = true
                deletedHere = false
            }.await()

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
                        if (account == mine && (e.code == "dropped" || e.code == GONE || e.code == "fork")) {
                            CoreLog.w("sync $name: ${e.code}")
                            if (e.code != "fork") forget()
                            problem = problemOf(e, SyncCopy.Action.Round)
                            // Said again at the next launch until OK: nobody may have seen it yet.
                            if (e.code == GONE) saveGone(true)
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
        fingerprint = null
        listFailed = false
        failed = Failed()
        lastReport = null
        lastSynced = null
        recovered = false
        saveRecovered(false)
        acked = emptySet()
        deletionCancelled = false
        dropKits()
    }

    companion object {
        private const val ROUNDS = 5

        /** The account was deleted, here or on another device. */
        const val GONE = "accountGone"

        /** How long a made kit's letters stay in memory after its page is left, before it counts as unseen. */
        const val KIT_HOLD_MS = 5 * 60_000L

        /** Codes after which the sync server will not let this device leave (the web's). */
        private val STUCK = setOf("fork", "wrongRelay", "relayBehind", "notJoined")

        fun stuck(code: String?) = code in STUCK

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
