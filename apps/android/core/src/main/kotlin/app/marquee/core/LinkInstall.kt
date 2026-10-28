package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreError
import app.marquee.core.model.AddonInfo
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

    /**
     * An addon link another app handed over (an Install button's stremio:// link): the link field takes
     * it and waits for the user's Add, never adding it by itself. Only in memory, like the field.
     */
    var offered: String? by mutableStateOf(null)

    /**
     * A link read by the core and waiting for the install sheet's answer (plans/addon-install-sheet.md):
     * [confirm] installs it, [cancel] drops it. The link may hold keys: memory only, never shown or logged.
     */
    class Checked(
        val url: String,
        val addon: AddonInfo,
        /** The user said a link with a sign-in is theirs: the install says so too. */
        val acknowledged: Boolean,
    ) {
        override fun toString() = "Checked(${addon.name})"
    }

    var checked: Checked? by mutableStateOf(null)
        private set

    val busy get() = said?.pending == true

    // Bumped for a new profile: a request from before must not land on the new profile's pages.
    private var generation = 0

    /**
     * Adds the addon [install] names (it returns the addon's name; its argument says whether the
     * user confirmed a link that carries a sign-in); [added] runs on success, for the field to clear.
     * A link refused for its sign-in leaves the warning; the user's confirm sends the field's link
     * again with [acknowledgeToken]. A second add while one runs does nothing (null).
     */
    fun run(
        install: suspend (acknowledgeToken: Boolean) -> String,
        added: () -> Unit = {},
        acknowledgeToken: Boolean = false,
    ): Job? {
        if (busy) return null
        val mine = generation
        said = Said("Adding the addon", failed = false, pending = true)
        return scope.launch {
            val after =
                try {
                    val name = install(acknowledgeToken)
                    if (mine == generation) added()
                    Said("Added $name", failed = false)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    refused(e, acknowledgeToken)
                }
            if (mine == generation) said = after
        }
    }

    /**
     * Reads [url] with [preview] before anything installs: "Checking" while it runs, then the sheet
     * ([checked]), or the sign-in warning or the failure under the field as an add says them. The
     * warning's confirm checks again with [acknowledgeToken]. Null while a request runs or a sheet is open.
     */
    fun check(
        url: String,
        acknowledgeToken: Boolean = false,
        preview: suspend (url: String, acknowledgeToken: Boolean) -> AddonInfo,
    ): Job? {
        if (busy || checked != null) return null
        val mine = generation
        said = Said(InstallSheetCopy.CHECKING, failed = false, pending = true)
        return scope.launch {
            val after =
                try {
                    val addon = preview(url, acknowledgeToken)
                    if (mine == generation) checked = Checked(url, addon, acknowledgeToken)
                    null
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    refused(e, acknowledgeToken)
                }
            if (mine == generation) said = after
        }
    }

    /** The sheet's Install: the checked link goes in with [install] (url, acknowledged), as [run] adds. */
    fun confirm(
        install: suspend (url: String, acknowledgeToken: Boolean) -> String,
        added: () -> Unit = {},
    ): Job? {
        val sure = checked ?: return null
        checked = null
        return run({ ack -> install(sure.url, ack) }, added, sure.acknowledged)
    }

    /** The sheet's Cancel and Back: nothing installs, the field keeps its link. */
    fun cancel() {
        checked = null
    }

    /** A failed add or check, in the core's words; a sign-in not yet acknowledged is the warning. */
    private fun refused(
        e: Exception,
        acknowledged: Boolean,
    ): Said =
        if (e is CoreError && e.code == TOKEN_IN_LINK && !acknowledged) {
            Said(TOKEN_WARNING, failed = true, token = true)
        } else {
            Said("Adding the addon did not work: ${InstallCards.wordsOf(e)}", failed = true)
        }

    /** The field changed after a sign-in warning: the warning was about the old link, so it goes. */
    fun edited() {
        if (said?.token == true) said = null
    }

    /** A new profile: nothing of the last one's add shows, and one still running lands nowhere. */
    fun forget() {
        generation++
        said = null
        checked = null
        offered = null
    }

    companion object {
        const val TOKEN_IN_LINK = "tokenInLink"
        const val TOKEN_WARNING =
            "This link carries a sign-in in its settings, maybe someone else's. Add it only if it is yours."
        const val TOKEN_CONFIRM = "It is mine, add it"
    }
}
