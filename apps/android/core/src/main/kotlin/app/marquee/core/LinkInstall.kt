package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreError
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
        /** The core refused the link because its settings carry a sign-in: [confirm] adds it anyway. */
        val token: Boolean = false,
    )

    var said: Said? by mutableStateOf(null)
        private set

    val busy get() = said?.pending == true

    // Bumped for a new profile: a request from before must not land on the new profile's pages.
    private var generation = 0

    /** The refused add, kept until the user confirms the link is theirs or starts another one. */
    private var refused: (() -> Job?)? = null

    /**
     * Adds the addon [install] names (it returns the addon's name; its argument is whether the user
     * confirmed a link that carries a sign-in); [added] runs on success, for the field to clear. A
     * second add while one runs does nothing (null).
     */
    fun run(
        install: suspend (acknowledgeToken: Boolean) -> String,
        added: () -> Unit = {},
    ): Job? = start(install, added, acknowledgeToken = false)

    /** The user says the link with a sign-in is theirs: it is sent again, acknowledged. */
    fun confirm(): Job? = refused?.invoke()

    private fun start(
        install: suspend (Boolean) -> String,
        added: () -> Unit,
        acknowledgeToken: Boolean,
    ): Job? {
        if (busy) return null
        val mine = generation
        refused = null
        said = Said("Adding the addon", failed = false, pending = true)
        return scope.launch {
            val after =
                try {
                    val name = install(acknowledgeToken)
                    if (mine == generation) added()
                    Said("Added $name", failed = false)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: CoreError) {
                    if (e.code == TOKEN_IN_LINK && !acknowledgeToken) {
                        if (mine == generation) refused = { start(install, added, acknowledgeToken = true) }
                        Said(TOKEN_WARNING, failed = true, token = true)
                    } else {
                        Said("Adding the addon did not work: ${InstallCards.wordsOf(e)}", failed = true)
                    }
                } catch (e: Exception) {
                    Said("Adding the addon did not work: ${InstallCards.wordsOf(e)}", failed = true)
                }
            if (mine == generation) said = after
        }
    }

    /** A new profile: nothing of the last one's add shows, and one still running lands nowhere. */
    fun forget() {
        generation++
        refused = null
        said = null
    }

    companion object {
        const val TOKEN_IN_LINK = "tokenInLink"
        const val TOKEN_WARNING =
            "This link carries a sign-in in its settings, maybe someone else's. Add it only if it is yours."
        const val TOKEN_CONFIRM = "It is mine, add it"
    }
}
