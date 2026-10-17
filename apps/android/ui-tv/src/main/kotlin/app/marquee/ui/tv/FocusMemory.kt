package app.marquee.ui.tv

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.focus.FocusRequester

/**
 * Where focus was on one screen, by stable string keys (`tile:<rowId>:<tileId>`), never by list
 * position: a row that gains items ahead of the saved one must not move the saved focus.
 *
 * The fields are plain, not Compose state, on purpose: a focus move only writes them, so nothing
 * recomposes (reading focus in a parent composable recomposed all of Home on every key press in
 * other TV apps). The one exception is [onFirstRow], which the Back chain reads and which flips
 * only when focus crosses between the first row and the rest.
 *
 * Instances are held by the screen's view model, so they outlive the composition: leaving a
 * screen for a title and coming back finds the key again.
 */
class FocusMemory {
    private val requesters = HashMap<String, FocusRequester>()
    private val chosenInRow = HashMap<String, String>()

    /** The key that last had focus on this screen, or null before any did. */
    var focusedKey: String? = null
        private set

    /** Counts focus arrivals. A restore that sees it change without having asked knows the user took over. */
    var arrivals = 0
        private set

    /** Set by the screen on every composition: the id of its first row. */
    var firstRow: String? = null

    /** True while focus is on a tile of the first row. Compose state, written only when it changes. */
    val onFirstRow = mutableStateOf(false)

    /** Focus arrived on [key], which belongs to the row [rowId] (or to a part of the page with no row). */
    fun arrived(
        rowId: String?,
        key: String,
    ) {
        focusedKey = key
        arrivals++
        if (rowId != null) chosenInRow[rowId] = key
        val first = rowId != null && rowId == firstRow
        if (onFirstRow.value != first) onFirstRow.value = first
    }

    /** Focus moved out of the content (to the rail). */
    fun leftContent() {
        if (onFirstRow.value) onFirstRow.value = false
    }

    fun attach(
        key: String,
        requester: FocusRequester,
    ) {
        requesters[key] = requester
    }

    fun detach(
        key: String,
        requester: FocusRequester,
    ) {
        if (requesters[key] === requester) requesters.remove(key)
    }

    /** The requester of a key whose node is composed right now. */
    fun requester(key: String): FocusRequester? = requesters[key]

    /** Where vertical entry into a row lands: the tile that row last had focused, if it is composed. */
    fun rowEntry(rowId: String): FocusRequester? = chosenInRow[rowId]?.let(requesters::get)

    /** Where entry into the screen from the rail lands. */
    fun lastEntry(): FocusRequester? = focusedKey?.let(requesters::get)
}

/**
 * What Home keeps while it is not on screen: scroll positions and focus memory. Lives in the
 * screen's view model; the lists are plain objects so their positions survive the composition.
 */
class TvHomeState {
    val focus = FocusMemory()
    val page = LazyListState()
    private val rows = HashMap<String, LazyListState>()

    fun rowState(rowId: String): LazyListState = rows.getOrPut(rowId) { LazyListState() }
}
