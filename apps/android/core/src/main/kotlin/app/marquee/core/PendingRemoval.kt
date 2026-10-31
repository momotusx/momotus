package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Tile
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * "Remove from Continue Watching" with [undoMs] to take it back. Home leaves the title out meanwhile; when
 * the time passes it goes to the core ([send], a synced register, so a slip would hide the title on every
 * device), and at once when another title is removed. A failure is said ([failed]) and the title comes back.
 */
class PendingRemoval(
    private val scope: CoroutineScope,
    private val undoMs: Long = UNDO_MS,
    private val send: suspend (Tile) -> Unit,
) {
    /** The title waiting for its Undo, which Home leaves out. */
    var removing: Tile? by mutableStateOf(null)
        private set

    /** What the last removal that failed says, for a while or until the next one. */
    var failed: String? by mutableStateOf(null)
        private set

    private var waiting: Job? = null

    /**
     * Leaves [tile] out and sends it after the Undo window; [longer] may stretch the window (the system's
     * "Time to take action", or no end at all while TalkBack is on before Android 10).
     */
    fun remove(
        tile: Tile,
        longer: (Long) -> Long = { it },
    ) {
        val earlier = removing
        waiting?.cancel()
        if (earlier != null) scope.launch { deliver(earlier) }
        failed = null
        removing = tile
        waiting =
            scope.launch {
                delay(longer(undoMs))
                deliver(tile)
            }
    }

    /**
     * The screens are going for good (Back out of the app, the process ending): a removal still waiting for
     * its Undo goes now on [outliving], a scope that lasts longer than this one, instead of being lost.
     */
    fun sendNow(outliving: CoroutineScope) {
        val tile = removing ?: return
        waiting?.cancel()
        waiting = null
        removing = null
        outliving.launch {
            try {
                send(tile)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("a removal sent on the way out failed: ${CoreLog.describe(e)}")
            }
        }
    }

    /** Undo: nothing goes to the core, and the title is back. */
    fun undo() {
        waiting?.cancel()
        waiting = null
        removing = null
    }

    /** [id] plays: a title that is wanted again is not removed, so its removal still waiting is dropped. */
    fun played(id: String) {
        if (removing?.id == id) undo()
    }

    /** A new profile: a removal still waiting is dropped, never sent for the next profile. */
    fun forget() {
        undo()
        failed = null
    }

    private suspend fun deliver(tile: Tile) {
        try {
            send(tile)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            val words = TileCopy.removeFailed(tile.name, InstallCards.wordsOf(e))
            failed = words
            // Said for a while, then the note goes.
            scope.launch {
                delay(undoMs * 2)
                if (failed == words) failed = null
            }
        } finally {
            // Gone from Home once the core's own list leaves it out; back when it failed.
            if (removing == tile) removing = null
        }
    }

    companion object {
        const val UNDO_MS = 5_000L
    }
}
