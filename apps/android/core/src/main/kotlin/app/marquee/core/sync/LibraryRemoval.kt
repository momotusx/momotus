package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.WipePending
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async

/**
 * "Remove your profiles and library from this device" (plans/accounts.md D13), the owner profile's only, in
 * three steps the app repeats after a restart: (1) the leave, with `removeAfter` so the core records the
 * removal before it goes out; (2) every profile signs out of Trakt and its record goes, and each debrid account
 * is disconnected tombstone first when asked; (3) the wipe, which the core refuses while either is left. The
 * core keeps the flag (`wipePending` in the profiles view): a start that finds it on the owner, with no account
 * here, finishes steps 2 and 3, so no crash leaves the previous owner's debrid key or Trakt sign-in behind.
 */
class LibraryRemoval(
    private val ops: CoreOps,
    private val sync: SyncAccount,
    private val scope: CoroutineScope,
    /** Signs each profile out of Trakt and drops its record; false when a record stayed. */
    private val traktSignOutEvery: suspend (List<String>) -> Boolean,
    /** Disconnects each debrid account the usual way (tombstone, core, record, tombstone). */
    private val debridDisconnectAll: suspend () -> Unit,
) {
    var running by mutableStateOf(false)
        private set

    /** Why the last run stopped, in words (Account offers Try again). */
    var problem: String? by mutableStateOf(null)
        private set

    /** The wipe went through: the app shows the first run, as on a new device. */
    var wiped by mutableStateOf(false)
        private set

    /** A removal running or done on this launch: the start does not run it again. */
    @Volatile
    private var resumed = false

    /** Sign out with removal: the leave first ([force]: also with edits unsent), then steps 2 and 3. */
    suspend fun signOutAndRemove(
        force: Boolean,
        debrid: Boolean,
        profiles: List<String>,
    ) = scope
        .async {
            resumed = true
            sync.signOut(force, removeAfter = debrid)
            steps(debrid, profiles)
        }.await()

    /**
     * The account-deleted card's Remove from this device: no account holds a leave, so the removal is recorded
     * first (`signOut {removeAfter}` without a record), then steps 2 and 3.
     */
    suspend fun removeHere(
        debrid: Boolean,
        profiles: List<String>,
    ) = scope
        .async {
            resumed = true
            guard { ops.signOut(record = null, removeAfter = debrid) }
            steps(debrid, profiles)
        }.await()

    /**
     * At start and after each profiles snapshot: a removal the core holds goes on, once per launch, only on the
     * owner profile of a device that holds no account.
     */
    fun resume(
        pending: WipePending?,
        owner: Boolean,
        profiles: List<String>,
    ) {
        if (pending == null) {
            resumed = false
            return
        }
        if (resumed || !owner || sync.joined || running) return
        resumed = true
        scope.async { runCatching { steps(pending.debrid, profiles) } }
    }

    /** Try again after a stopped run. */
    suspend fun retry(
        debrid: Boolean,
        profiles: List<String>,
    ) = scope.async { steps(debrid, profiles) }.await()

    private suspend fun steps(
        debrid: Boolean,
        profiles: List<String>,
    ) {
        running = true
        problem = null
        // Each removal ends in its own `wiped`, which the app answers once (the first run, as on a new device).
        wiped = false
        try {
            guard {
                if (!traktSignOutEvery(profiles)) CoreLog.w("removal: a Trakt record stayed; the wipe will say so")
                if (debrid) debridDisconnectAll()
                ops.signOut(record = null, wipe = true, disconnectDebrid = debrid)
            }
            wiped = true
        } finally {
            running = false
        }
    }

    private suspend fun guard(block: suspend () -> Unit) {
        try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            val code = (e as? CoreError)?.code
            problem =
                when (code) {
                    "debridConnected" -> AccountCopy.DEBRID_CONNECTED
                    "traktConnected" -> AccountCopy.TRAKT_CONNECTED
                    "notOwner" -> AccountCopy.NOT_OWNER
                    else -> AccountCopy.WIPE_FAILED
                }
            CoreLog.w("removal stopped: ${code ?: e.javaClass.simpleName}")
            throw e
        }
    }
}
