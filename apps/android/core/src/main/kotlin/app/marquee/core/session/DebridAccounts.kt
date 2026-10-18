package app.marquee.core.session

import app.marquee.core.client.CoreLog
import app.marquee.core.model.DebridRecord
import app.marquee.core.secrets.DebridCore
import app.marquee.core.secrets.DebridStore
import app.marquee.core.secrets.LegacyDebridKeystore
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope

/**
 * The device's debrid accounts for the app's life (plans/c10-handoff.md, phase 3a, revised): one
 * [DebridStore] across every core the process opens.
 *
 * Each core open runs [opened] before any screen may open a view: the helper's `start()` (finish
 * interrupted disconnects, `restoreDebrid`), then what the `debrid` view already holds, then the move
 * of legacy keys. Pushes of the view reach the helper only once that core's `start()` returned.
 */
class DebridAccounts(
    private val keystore: LegacyDebridKeystore,
    core: DebridCore,
    /** `setDebridKey`: one connection by key; its record arrives through the view. */
    private val connect: suspend (service: String, key: String) -> Unit,
    scope: CoroutineScope,
) {
    val store = DebridStore(keystore, core, scope)

    @Volatile
    private var ready = false

    /** The core is going away: its views' pushes wait for the next core's restore. */
    fun pause() {
        ready = false
    }

    /**
     * A core opened. [current] reads the pending records the `debrid` view holds now. A restore that
     * fails twice is logged and the app goes on: records stay in the keystore for the next start, and
     * new connections are still stored.
     */
    suspend fun opened(current: () -> List<DebridRecord>?) {
        ready = false
        // What the last core left queued is stored first: the restore then reads the newest record
        // of each service, never one a queued write was about to replace.
        store.idle()
        for (attempt in 1..2) {
            try {
                store.start()
                break
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("debrid restore failed (attempt $attempt): ${CoreLog.describe(e)}")
            }
        }
        ready = true
        current()?.let { store.pending(it) }
        val legacy = keystore.legacy().size
        CoreLog.d("debrid: ${keystore.records().size} records restored, $legacy legacy keys to move")
        moveLegacy()
    }

    /** Every push of the `debrid` view. */
    fun pushed(pending: List<DebridRecord>) {
        if (ready) store.pending(pending)
    }

    /**
     * Keys from before records, one at a time: each becomes a connection whose record, once stored,
     * drops the key. A failure stops here; the rest moves at the next start.
     */
    private suspend fun moveLegacy() {
        for ((service, key) in keystore.legacy()) {
            try {
                connect(service, key)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("a legacy $service key did not move yet: ${CoreLog.describe(e)}")
                return
            }
        }
    }
}
