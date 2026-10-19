package app.marquee.core.trakt

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
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

    fun delete(profile: String)
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

    override fun delete(profile: String) {
        secrets.update { it.copy(traktRecords = it.traktRecords - profile) }
    }
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

    /** The last record could be kept in memory only: after a restart this device signs in again. */
    var memoryOnly by mutableStateOf(false)
        private set

    @Volatile
    private var profile: String? = null
    private var view: AutoCloseable? = null

    /** The record last handed over as an acknowledgement, so a push repeating it is not stored twice. */
    @Volatile
    private var acked: KeystoreRecord? = null
    private val line = Mutex()

    /** [profile] runs, unlocked: hand the core its record, then follow its status. */
    fun begin(profile: String) {
        end()
        this.profile = profile
        scope.launch {
            line.withLock {
                if (this@TraktAccount.profile != profile) return@withLock
                try {
                    ops.setTrakt(records.load(profile))
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    // A build without Trakt answers notConfigured; the view says so.
                    CoreLog.w("trakt: the record was not handed over: ${CoreLog.describe(e)}")
                }
                synchronized(this@TraktAccount) {
                    if (this@TraktAccount.profile == profile) view = openView { pushed(profile, it) }
                }
            }
        }
    }

    /** The profile stops (a switch, a lock, a core that stopped): its view closes. */
    @Synchronized
    fun end() {
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
        val record = next.record ?: return
        if (record == acked) return
        acked = record
        scope.launch {
            line.withLock {
                // Stored whatever runs now: it is the newest record of that profile's account.
                memoryOnly = !records.save(owner, record)
                // ponytail: checked once before the send; a switch waits for this view to close first.
                if (profile != owner) return@withLock
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

    /** One round now. */
    suspend fun syncNow(): TraktReport = scope.async { ops.traktSync() }.await()

    /** Signs this device out for the running profile, then deletes its record; a refusal keeps it. */
    suspend fun signOut() =
        scope
            .async {
                line.withLock {
                    val owner = profile ?: return@withLock
                    ops.traktSignOut()
                    records.delete(owner)
                    acked = null
                    memoryOnly = false
                }
            }.await()
}
