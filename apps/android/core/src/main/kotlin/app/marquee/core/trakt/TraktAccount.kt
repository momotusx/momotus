package app.marquee.core.trakt

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreLog
import app.marquee.core.model.KeystoreRecord
import app.marquee.core.model.TraktReport
import app.marquee.core.model.TraktStatus
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Where each profile's Trakt record lives. */
interface TraktRecords {
    fun load(profile: String): KeystoreRecord?

    /** False when the record could be kept in memory only, for this session. */
    fun save(
        profile: String,
        record: KeystoreRecord,
    ): Boolean

    /** False when the file could not drop it: the record stays where it was. */
    fun delete(profile: String): Boolean
}

/** The encrypted secrets file: this device only, its key in the Android Keystore, never backed up. */
class SecretTraktRecords(
    private val secrets: SecretStore,
) : TraktRecords {
    override fun load(profile: String) = secrets.load().traktRecords[profile]

    override fun save(
        profile: String,
        record: KeystoreRecord,
    ): Boolean {
        secrets.update { it.copy(traktRecords = it.traktRecords + (profile to record)) }
        return secrets.health != SecretStore.Health.MemoryOnly
    }

    // Durable, as a debrid disconnect: a delete that stays in memory would come back at the next start.
    override fun delete(profile: String) = secrets.updateDurably { it.copy(traktRecords = it.traktRecords - profile) }
}

/**
 * The session profile's Trakt account (plans/m7-trakt.md section 3), for the app's life.
 *
 * [begin] runs each time a profile's content may show: the core opened, the session was unlocked, the
 * profile switched. The profile's stored record goes to the core first (`setTrakt` is refused while the
 * session is locked), then the `trakt` view opens for as long as that profile runs; [end] closes it before a
 * switch or a lock. Every record the view offers is stored before it is acknowledged with `setTrakt`, and
 * only the profile it came from is ever handed it. Ops run one at a time in [scope], the app's: leaving a
 * page never cuts one between the core's answer and the keystore.
 */
class TraktAccount(
    private val ops: CoreOps,
    private val records: TraktRecords,
    private val scope: CoroutineScope,
    /** Opens the `trakt` view, its pushes going to the callback; closing the handle closes the view. */
    private val openView: ((TraktStatus) -> Unit) -> AutoCloseable,
) {
    /** The running profile's status, from its `trakt` view; null before the first push. */
    var status: TraktStatus? by mutableStateOf(null)
        private set

    /** The running profile's record could be kept in memory only: after a restart this device signs in again. */
    var memoryOnly by mutableStateOf(false)
        private set

    /** Profiles whose last record stayed in memory only, so [memoryOnly] follows the profile that runs. */
    private val inMemory = HashSet<String>()

    @Volatile
    private var profile: String? = null
    private var view: AutoCloseable? = null

    /** Each begin and end takes a turn: only the newest begin hands over and opens a view. */
    @Volatile
    private var turns = 0

    /** The record last handed over as an acknowledgement, so a push repeating it is not stored twice. */
    @Volatile
    private var acked: KeystoreRecord? = null
    private val line = Mutex()

    /**
     * [profile] runs, unlocked: hand the core its record, then follow its status. With no record here
     * nothing is sent: the core keeps what it holds (a sign-in whose record is still on its way).
     */
    fun begin(profile: String) {
        // Trakt never runs in Incognito (plans/batch-10-incognito.md 5.6): no record goes over, no view opens.
        if (profile == ProfileCopy.INCOGNITO) return end()
        val turn =
            synchronized(this) {
                end()
                this.profile = profile
                turns
            }
        scope.launch {
            line.withLock {
                if (turns != turn) return@withLock
                memoryOnly = profile in inMemory
                records.load(profile)?.let { record ->
                    try {
                        ops.setTrakt(record)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        // A build without Trakt answers notConfigured; the view says so.
                        CoreLog.w("trakt: the record was not handed over: ${CoreLog.describe(e)}")
                    }
                }
                synchronized(this@TraktAccount) {
                    if (turns == turn) view = openView { pushed(profile, it) }
                }
            }
        }
    }

    /** The profile stops (a switch, a lock, a core that stopped): its view closes. */
    @Synchronized
    fun end() {
        turns++
        profile = null
        acked = null
        status = null
        view?.close()
        view = null
    }

    private fun pushed(
        owner: String,
        next: TraktStatus,
    ) {
        if (profile == owner) status = next
        // The stored record is no longer this device's (disconnected for the account, or the account was left).
        if (next.dropStored && next.record == null) return dropStored(owner)
        val record = next.record ?: return
        if (record == acked) return
        acked = record
        scope.launch {
            line.withLock {
                // Stored whatever runs now: it is the newest record of that profile's account.
                val stored = records.save(owner, record)
                if (stored) inMemory -= owner else inMemory += owner
                // The check and the send are one step under the lock: a switch first takes the lock too
                // ([quiet]), so a record sent here reaches the core before the switch, never after it.
                if (profile != owner) return@withLock
                memoryOnly = !stored
                try {
                    ops.setTrakt(record)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    // Offered again with the next push, and handed over at the next begin anyway.
                    acked = null
                    CoreLog.w("trakt: a stored record was not acknowledged: ${CoreLog.describe(e)}")
                }
            }
        }
    }

    /** Deletes [owner]'s record from the file, once per push of `dropStored`; deleting what is gone is nothing. */
    private fun dropStored(owner: String) {
        acked = null
        scope.launch {
            line.withLock {
                if (records.load(owner) == null) return@withLock
                inMemory -= owner
                if (profile == owner) memoryOnly = false
                if (!records.delete(owner)) CoreLog.w("trakt: a record the core dropped could not be removed yet")
            }
        }
    }

    /**
     * Waits until no record is being handed to the core. The profile switch calls it after [end] and before
     * `switchProfile`: a send under way reaches the core first, and later ones see the profile has left.
     */
    suspend fun quiet() = line.withLock {}

    /** One round now. */
    suspend fun syncNow(): TraktReport = scope.async { ops.traktSync() }.await()

    /**
     * Signs this device out for the running profile, then deletes its record from the file. False when
     * the file could not drop it: the core is signed out, and the record would come back at the next
     * start, so the page says so and offers to try again. A refusal from the core throws and keeps it.
     */
    suspend fun signOut(): Boolean =
        scope
            .async {
                line.withLock {
                    val owner = profile ?: return@withLock true
                    ops.traktSignOut()
                    acked = null
                    inMemory -= owner
                    memoryOnly = false
                    records.delete(owner)
                }
            }.await()

    /**
     * D13's step 2 (plans/accounts.md): every profile on this device drops its Trakt sign-in, its record leaves
     * the file. Local only (plans/trakt-account.md 3.5): no revoke, so the account's other devices stay
     * connected. The session's own profile goes as itself; the others are the owner's to end (`notOwner` from
     * any other profile). False when a record could not be dropped (the wipe then waits for a retry).
     */
    suspend fun signOutEvery(profiles: List<String>): Boolean =
        scope
            .async {
                line.withLock {
                    var dropped = true
                    for (p in profiles) {
                        ops.traktSignOut(if (p == profile) null else p, local = true)
                        inMemory -= p
                        if (!records.delete(p)) dropped = false
                    }
                    acked = null
                    memoryOnly = false
                    dropped
                }
            }.await()

    /**
     * A profile was deleted, here or by sync (`{"profile": "gone"}`, also sent at start for one removed
     * meanwhile): its record leaves the file. The core revokes and drops its own state; a delete that
     * fails is tried again at the next start, when the core says it is gone again.
     */
    fun forget(profile: String) {
        scope.launch {
            line.withLock {
                if (!records.delete(profile)) CoreLog.w("trakt: a deleted profile's record could not be removed yet")
            }
        }
    }
}
