package app.marquee.ui.tv

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.layout.LazyLayoutCacheWindow
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
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

    /**
     * Counts D-pad presses (arrows and select). A restore that sees it change knows the user took
     * over. Focus arrivals do not count: the system gives the window a default focus of its own
     * when it appears, before any key is pressed.
     */
    var interactions = 0
        private set

    /** True while the node of [focusedKey] has focus (it keeps the key when focus moves elsewhere). */
    var hasFocus = false
        private set

    private var focusedRow: String? = null

    /** The id of the screen's first row; the screen sets it on every composition and calls [refresh]. */
    var firstRow: String? = null

    /** True while focus is on a tile of the first row. Compose state, written only when it changes. */
    val onFirstRow = mutableStateOf(false)

    /**
     * True while another screen is on top of this one (Home stays composed under a title page during
     * the fade). Arrivals are not recorded then: when the page above drops its focused node, the system
     * hands focus to the first node of the page below, and that must not become where Back returns to.
     */
    var paused = false

    /** Focus arrived on [key], which belongs to the row [rowId] (or to a part of the page with no row). */
    fun arrived(
        rowId: String?,
        key: String,
    ) {
        if (paused) return
        focusedKey = key
        focusedRow = rowId
        hasFocus = true
        if (rowId != null) chosenInRow[rowId] = key
        refresh()
    }

    /** The node of [key] lost focus. */
    fun lost(key: String) {
        if (focusedKey != key) return
        hasFocus = false
        refresh()
    }

    /** Recomputes [onFirstRow]; also needed when a lane is inserted above the focused one. */
    fun refresh() {
        val first = hasFocus && focusedRow != null && focusedRow == firstRow
        if (onFirstRow.value != first) onFirstRow.value = first
    }

    /** A D-pad key went down (called by the shell). */
    fun interacted() {
        interactions++
    }

    /** Focus moved out of the content (to the pill). */
    fun leftContent() {
        hasFocus = false
        refresh()
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

    /** Where entry into the screen from the pill lands. */
    fun lastEntry(): FocusRequester? = focusedKey?.let(requesters::get)
}

/** A requester that is registered under [key] while its node is composed, so focus can be sent to it by key. */
@Composable
fun FocusMemory.requesterFor(key: String): FocusRequester {
    val requester = remember(key) { FocusRequester() }
    DisposableEffect(key, requester) {
        attach(key, requester)
        onDispose { detach(key, requester) }
    }
    return requester
}

/** The callback for a node's focus changes: it tells this memory where focus is. */
fun FocusMemory.watch(
    rowId: String?,
    key: String,
): (Boolean) -> Unit = { gained -> if (gained) arrived(rowId, key) else lost(key) }

/**
 * What Home keeps while it is not on screen: scroll positions and focus memory. Lives in the
 * screen's view model; the lists are plain objects so their positions survive the composition.
 */
class TvHomeState(
    /** A device short of memory (a TV stick, [isLowMemoryDevice]): Home keeps fewer lanes composed ahead. */
    lowMemory: Boolean = false,
) {
    val focus = FocusMemory()

    /** How far ahead of the screen the page composes lanes, in screens: see [page]. */
    internal val ahead = aheadFraction(lowMemory)

    /**
     * The lanes ahead are composed while the page is idle, tile by tile: a Down brings the next lane into view
     * within its first frames, too soon for the default prefetch, which starts with the scroll, so the lane's
     * tiles composed inside those frames. A screen ahead and 0.3 behind (for a Down right after an Up). On the
     * top shelf layout every Down finds its lane composed from 0.4 ahead, from the hero too, but not at 0.9 nor
     * below 0.4 (TvHomeScrollTest): the margin is not monotonic, so a change here needs those tests.
     * ponytail: on the Pixel in TV form the window cost +21.9 MB PSS at Row 5 (bench/android/scroll.jsonl,
     * 2026-10-23), so a low-memory device keeps 0.5 ahead; none at all there if a stick still runs short.
     */
    @OptIn(ExperimentalFoundationApi::class)
    val page = LazyListState(LazyLayoutCacheWindow(aheadFraction = ahead, behindFraction = 0.3f))
    private val rows = HashMap<String, LazyListState>()

    fun rowState(rowId: String): LazyListState = rows.getOrPut(rowId) { LazyListState() }

    /** Home shows a card between its lanes (the recommended addons card): the lanes under it sit one item lower. */
    var cardShown = false
}

/** How far ahead of the screen Home composes lanes, in screens: see [TvHomeState.page]. */
internal fun aheadFraction(lowMemory: Boolean): Float = if (lowMemory) 0.5f else 1f

/**
 * A device short of memory, where Home composes fewer lanes ahead: one the system calls low on RAM, or one
 * that gives an app [memoryClassMb] or less of heap, which most TV sticks do.
 */
fun isLowMemoryDevice(
    lowRamDevice: Boolean,
    memoryClassMb: Int,
): Boolean = lowRamDevice || memoryClassMb <= LOW_MEMORY_CLASS_MB

private const val LOW_MEMORY_CLASS_MB = 192

/**
 * A screen's first focus: on [target] once there is one (it often comes late, with the data), or on
 * the key this screen last had focused while that is still composed. Never after a D-pad press since
 * the screen asked (on entry, or when [ask] changes, as on Try again): the viewer's move stands even
 * when the target came after it, as on the web (d8aa39f1).
 */
@Composable
fun FocusMemory.FirstFocus(
    target: String?,
    ask: Any? = null,
) {
    val asked = remember(this, ask) { interactions }
    LaunchedEffect(this, target, asked) {
        if (target == null) return@LaunchedEffect
        // A remembered key that is no longer composed (a row that moved into the card) is not a target.
        val wanted = focusedKey?.takeIf { requester(it) != null } ?: target
        focusWhenReady(wanted, asked, waitMs = 10_000)
    }
}

/**
 * Puts focus on [key] as soon as its node exists, trying once per frame for up to [waitMs]; stops
 * when it lands or when a key went down since [asked] (the count of [FocusMemory.interactions] when
 * the focus was asked for). For screens whose targets are always composed (a title page); Home
 * scrolls its lists first and has its own routine.
 */
suspend fun FocusMemory.focusWhenReady(
    key: String,
    asked: Int,
    waitMs: Long = 4_000,
) {
    val started = withFrameNanos { it }
    while ((withFrameNanos { it } - started) / 1_000_000 < waitMs) {
        if (interactions != asked) return
        val requester = requester(key) ?: continue
        try {
            requester.requestFocus()
        } catch (e: IllegalStateException) {
            continue // not attached yet
        }
        if (focusedKey == key && hasFocus) return
    }
}
