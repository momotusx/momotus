package app.marquee.core

import androidx.compose.runtime.mutableStateMapOf
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Installed
import app.marquee.core.model.ListedAddon
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * Each Find addons card's Installing and failure state, by entry key (the web's
 * `lib/install-cards.ts`). It lives outside the page, so a result that lands after a list switch,
 * or after the page was closed and opened again, updates only its own card. Keys may hold an
 * addon's keys: they stay in memory and are never shown or logged.
 *
 * Requests run on [scope] (the app's, not the page's), and the state is written there.
 */
class InstallCards(
    private val scope: CoroutineScope,
) {
    data class State(
        val busy: Boolean,
        /** The core's failure code (`timeout`, `needsSetup`, ...), null on success or without one. */
        val code: String? = null,
        /** The failure in the core's words. */
        val words: String? = null,
    )

    private val states = mutableStateMapOf<String, State>()

    // Bumped when every card is forgotten for a new profile: a request still running from before
    // must not land on the new profile's card.
    private var generation = 0

    private val seen = HashMap<String, Installed>()

    operator fun get(key: String): State? = states[key]

    /** Runs one card's request; a second press while it runs does nothing (null). */
    fun run(
        key: String,
        request: suspend () -> Unit,
    ): Job? {
        if (states[key]?.busy == true) return null
        val mine = generation
        states[key] = State(busy = true)
        return scope.launch {
            var after = State(busy = false)
            try {
                request()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                after = State(busy = false, code = (e as? CoreError)?.code, words = e.message ?: CoreLog.describe(e))
            } finally {
                if (mine == generation) states[key] = after
            }
        }
    }

    /** Forgets a card's last result (its entry's installed state changed); a running one stays. */
    fun forget(key: String) {
        if (states[key]?.busy == false) states.remove(key)
    }

    /**
     * Forgets every card's result. For a new profile ([running] true) also those still running, whose
     * results then land nowhere; for Try again and Refresh (false) a running request keeps its card.
     */
    fun forgetAll(running: Boolean = true) {
        if (running) {
            generation++
            states.clear()
            seen.clear()
        } else {
            states.keys.filter { states[it]?.busy == false }.forEach(states::remove)
        }
    }

    /**
     * A new snapshot: a result is about the state it was pressed in, so an entry whose installed
     * state changed (from this card or another device) loses its card's result.
     */
    fun follow(entries: List<ListedAddon>) {
        for (entry in entries) {
            val before = seen.put(entry.key, entry.installed)
            if (before != null && before != entry.installed) forget(entry.key)
        }
    }
}
