package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * Adding an addon by its link, the one flow behind Settings' field and Find addons' Paste a link. It
 * runs on [scope] (the app's, not the page's), so leaving the page does not cancel it, and what it
 * ended with is there when either page opens again. A new profile forgets it: a request still running
 * then lands nowhere. The link may hold keys: it is never kept or logged.
 */
class LinkInstall(
    private val scope: CoroutineScope,
) {
    /** What the last add ended with, or that one is under way. */
    data class Said(
        val words: String,
        val failed: Boolean,
        val pending: Boolean = false,
    )

    var said: Said? by mutableStateOf(null)
        private set

    val busy get() = said?.pending == true

    // Bumped for a new profile: a request from before must not land on the new profile's pages.
    private var generation = 0

    /**
     * Adds the addon [install] names (it returns the addon's name); [added] runs on success, for the
     * field to clear. A second add while one runs does nothing (null).
     */
    fun run(
        install: suspend () -> String,
        added: () -> Unit = {},
    ): Job? {
        if (busy) return null
        val mine = generation
        said = Said("Adding the addon", failed = false, pending = true)
        return scope.launch {
            val after =
                try {
                    val name = install()
                    if (mine == generation) added()
                    Said("Added $name", failed = false)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Said("Adding the addon did not work: ${InstallCards.wordsOf(e)}", failed = true)
                }
            if (mine == generation) said = after
        }
    }

    /** A new profile: nothing of the last one's add shows, and one still running lands nowhere. */
    fun forget() {
        generation++
        said = null
    }
}
