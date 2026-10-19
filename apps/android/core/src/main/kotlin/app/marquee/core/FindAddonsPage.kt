package app.marquee.core

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.FindAddons.Action
import app.marquee.core.model.ListedAddon
import app.marquee.core.session.CoreOps
import app.marquee.core.views.AddonCatalogView
import kotlinx.coroutines.Job

/**
 * Find addons apart from its screen: the open `addonCatalog` view, Refresh and Try again, and what a
 * card's press sends. [open] opens the view (`fresh` waits for the network instead of the addon
 * cache); the cards' states live in [cards], outside the page.
 */
class FindAddonsPage(
    private val ops: CoreOps,
    private val cards: InstallCards,
    private val open: (fresh: Boolean) -> AddonCatalogView,
) : AutoCloseable {
    var view: AddonCatalogView by mutableStateOf(open(false))
        private set

    /** Refresh and Try again: the view opens again with `fresh`; results from before go, running cards stay. */
    fun retry() {
        cards.forgetAll(running = false)
        view.close()
        view = open(true)
    }

    /** Installs a listed entry, or turns on the link its action names (a configured copy's for copyOff). */
    fun press(
        entry: ListedAddon,
        action: Action,
    ): Job? =
        when (action) {
            is Action.Install -> cards.run(entry.key) { ops.installListed(entry.key) }
            is Action.TurnOn -> cards.run(entry.key) { ops.setAddonEnabled(action.url, true) }
            else -> null
        }

    override fun close() = view.close()
}
