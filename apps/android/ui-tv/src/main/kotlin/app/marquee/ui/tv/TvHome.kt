@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.PendingRemoval
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.model.allFailed
import app.marquee.core.model.notice
import app.marquee.core.model.playable
import app.marquee.core.model.tileKeys
import app.marquee.core.model.withoutRemoved
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.ActionMenu
import app.marquee.design.Backdrop
import app.marquee.design.Icons
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.RatingLine
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.Scrims
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileMetrics
import app.marquee.design.TileShape
import app.marquee.design.actionWindow
import app.marquee.design.describe
import app.marquee.design.focusSoon
import app.marquee.design.laneHeight
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

private const val HERO_PLAY = "hero:play"
private const val HERO_DETAILS = "hero:details"
private const val HERO_ROW = "hero"
private val HERO_HEIGHT = 560.dp

/** The hero's text column, 56 in from the card's edge: inside Scrims.heroSide's dark side. */
private val HERO_TEXT = 640.dp
private const val FOCUS_WAIT_MS = 15_000L

private fun Hero?.focusKey(): String? =
    when {
        this == null -> null
        tile.playable() -> HERO_PLAY
        else -> HERO_DETAILS
    }

/**
 * Home on TV: a hero, then one lane per catalog. Focus starts on the first tile of Continue
 * Watching when there is one, else on the hero's Play, comes back to the exact tile after a title
 * page, and enters each lane on the tile that lane last had focused.
 *
 * @param onOpen a tile or Details was chosen: the title page.
 * @param onPlay the hero's Play was chosen.
 */
@Composable
fun TvHome(
    home: HomeView,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    modifier: Modifier = Modifier,
    /** Catalogs exist and every one is hidden: the empty state points to Home rows, not setup. */
    rowsHidden: Boolean = false,
    onHomeRows: () -> Unit = {},
    /** A one-time card (new recommended addons): under Continue Watching, or first when there is none. */
    card: (@Composable () -> Unit)? = null,
    /**
     * "Remove from Continue Watching", from a held select on one of its tiles, with its Undo under the row;
     * null offers no menu.
     */
    removal: PendingRemoval? = null,
) {
    val safe = Marquee.safe
    val metrics = Marquee.tiles
    // The held tile and its key, while its menu is open.
    var menuFor by remember { mutableStateOf<Pair<Tile, String>?>(null) }
    val scope = rememberCoroutineScope()
    val accessibility = LocalAccessibilityManager.current
    val undoFocus = remember { FocusRequester() }
    // After Remove: where focus goes when the note leaves. The next tile (the next one, the one before, or the
    // next row) when the removal went through; the title itself after Undo or a failure, when it is back.
    var leaving by remember { mutableStateOf<Pair<String?, String>?>(null) }
    var undone by remember { mutableStateOf(false) }
    // The D-pad presses counted when the viewer last acted on the removal (Remove, or Undo): one after that
    // means they moved on, and their move stands.
    var actedAt by remember { mutableStateOf(0) }
    menuFor?.let { (tile, key) ->
        ActionMenu(
            title = tile.name,
            actions =
                listOf(
                    MenuAction(TileCopy.REMOVE_CONTINUE, Icons.X) {
                        leaving = focusAfterRemove(home.rows.toList(), key) to key
                        undone = false
                        actedAt = state.focus.interactions
                        removal?.remove(tile, accessibility::actionWindow)
                        // Undo is the one thing the viewer may want now: it takes the focus at once.
                        scope.launch { focusSoon(undoFocus, UNDO_FOCUS_FRAMES) }
                    },
                ),
            onDismiss = { menuFor = null },
        )
    }
    val hold: ((Tile, String) -> Unit)? = removal?.let { { tile, key -> menuFor = tile to key } }
    // A failed row's Try again: the row turns to a skeleton and its focused button goes, so focus goes to the
    // row's first tile when the tiles come, unless the viewer has moved on by then.
    val retry: (String) -> Unit = { rowId ->
        home.retryRow(rowId)
        scope.launch {
            val seen = state.focus.interactions
            val first =
                withTimeoutOrNull(FOCUS_WAIT_MS) {
                    snapshotFlow {
                        home.rows
                            .toList()
                            .firstOrNull { it.rowId == rowId }
                            ?.tileKeys()
                            ?.firstOrNull()
                    }.filterNotNull()
                        .first()
                }
            if (first != null && state.focus.interactions == seen) restoreFocus(home, state, first)
        }
    }
    val removing = removal?.removing
    LaunchedEffect(removing) {
        val (next, held) = leaving ?: return@LaunchedEffect
        if (removing != null) return@LaunchedEffect
        leaving = null
        // Not after the viewer moved on (the note going lets focus fall on a neighbour by itself, so where
        // focus is now says nothing).
        if (state.focus.interactions != actedAt) return@LaunchedEffect
        val back = if (undone || removal?.failed != null) held else next
        if (back != null) restoreFocus(home, state, back)
    }
    // Read from the snapshot, not in a SideEffect: lanes arrive without recomposing this function,
    // and a stale start row would make every Back refocus the same tile.
    LaunchedEffect(state) {
        snapshotFlow { startRow(home.rows.toList(), home.hero) }.collect {
            state.focus.firstRow = it
            state.focus.refresh()
        }
    }
    LaunchedEffect(state) { restoreFocus(home, state, state.focus.focusedKey) }
    // On the first lane the hero shows above it: the page rests at its top. This waits for the
    // scroll that brings the focused tile into view to end, and runs again when the hero arrives
    // (the list keeps the first lane in place, so a hero that arrives later starts out of view).
    val hasHero = home.hero != null
    LaunchedEffect(state, hasHero) {
        // Nobody has scrolled yet: a hero that arrives now belongs in view.
        if (hasHero && state.page.firstVisibleItemIndex <= 1 && state.page.firstVisibleItemScrollOffset == 0) {
            state.page.scrollToItem(0)
        }
        snapshotFlow { state.focus.onFirstRow.value }.collectLatest { onFirst ->
            if (!onFirst) return@collectLatest
            withFrameNanos { }
            withFrameNanos { }
            snapshotFlow { state.page.isScrollInProgress }.first { !it }
            if (state.focus.onFirstRow.value) state.page.animateScrollToItem(0)
        }
    }

    // A focused row rests under the pill with its header, never behind it: a focused tile comes to rest
    // its header's room lower than the content's top (the lanes keep their own leading edge, TileLane).
    var headerRoom by remember { mutableStateOf(0.dp) }
    AnchoredScrolling(lead = tvContentTop() + headerRoom, trail = 0.dp) {
        LazyColumn(
            state = state.page,
            modifier = modifier.fillMaxSize(),
            contentPadding = PaddingValues(top = tvContentTop(), bottom = safe.bottom),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            val hero = home.hero
            if (hero != null) item(key = "hero") { TvHero(hero, state, onOpen, onPlay) }
            if (home.rows.isEmpty()) item(key = "state") { HomeState(home, onSetup, onAddons, rowsHidden, onHomeRows) }
            if (hero == null && home.rows.toList().allFailed()) {
                item(key = "failed") { AllFailed(home, Modifier.padding(start = Marquee.safe.side)) }
            } else {
                val rows = home.rows.toList().withoutRemoved(removing?.id)
                val split = cardSplit(rows)
                state.cardShown = card != null
                items(
                    rows.subList(0, split),
                    key = { it.rowId },
                ) { row -> TvRow(row, TileCopy.source(row, rows), state, onOpen, hold, retry, { headerRoom = it }) }
                val said = removing?.let { TileCopy.removed(it.name) } ?: removal?.failed
                if (said != null) {
                    item(key = "notice") {
                        RemovalNote(said, removing != null, undoFocus, state) {
                            undone = true
                            actedAt = state.focus.interactions
                            removal?.undo()
                        }
                    }
                }
                if (card != null) item(key = "card") { card() }
                items(
                    rows.subList(split, rows.size),
                    key = { it.rowId },
                ) { row -> TvRow(row, TileCopy.source(row, rows), state, onOpen, hold, retry, { headerRoom = it }) }
            }
        }
    }
}

@Composable
private fun HomeState(
    home: HomeView,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    rowsHidden: Boolean,
    onHomeRows: () -> Unit,
) {
    val inset = Marquee.safe.side
    when (val status = home.status) {
        is ViewStatus.Failed -> {
            StateMessage(
                title = "Home could not load",
                body = status.reason,
                icon = Icons.CircleAlert,
                actionLabel = "Try again",
                onAction = home::retry,
                modifier = Modifier.padding(start = inset, top = Marquee.safe.top),
            )
        }

        ViewStatus.Opening -> {
            SkeletonLane(TileShape.Poster)
        }

        ViewStatus.Open -> {
            if (rowsHidden) {
                // Catalogs exist and every one is hidden: setup would change nothing.
                StateMessage(
                    title = "Every Home row is hidden",
                    body =
                        "Your addons have catalogs, and each one is hidden from Home. " +
                            "Show them again in Home rows.",
                    actionLabel = "Show rows",
                    onAction = onHomeRows,
                    modifier = Modifier.padding(start = inset, top = Marquee.safe.top),
                )
                return
            }
            // Open with no rows: no addon that is on has a catalog, or every row is hidden.
            StateMessage(
                title = "Home fills up from your addons",
                body =
                    "None of the addons you have on shows a catalog here. " +
                        "Set up Marquee offers the recommended addons in one step, or add an addon yourself.",
                actionLabel = "Set up Marquee",
                onAction = onSetup,
                secondaryLabel = "Add an addon",
                onSecondary = onAddons,
                modifier = Modifier.padding(start = inset, top = Marquee.safe.top),
            )
        }
    }
}

/** Every lane failed and none has a tile (most often no network): one way on instead of a column of failures. */
@Composable
private fun AllFailed(
    home: HomeView,
    modifier: Modifier = Modifier,
) {
    StateMessage(
        title = "Home could not load",
        body = "None of your addons answered. Check the network, then try again.",
        icon = Icons.CircleAlert,
        actionLabel = "Try again",
        onAction = home::retry,
        modifier = modifier,
    )
}

@Composable
private fun TvHero(
    hero: Hero,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
) {
    val colors = Marquee.colors
    val tile = hero.tile
    val shape = RoundedCornerShape(Marquee.radius.hero)
    val playRequester = state.focus.requesterFor(HERO_PLAY)
    val detailsRequester = state.focus.requesterFor(HERO_DETAILS)
    // An inset card (design/DECISION.md v2, Home hero): the backdrop with scrims from the left and
    // the bottom, so the words read on any artwork.
    Box(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .height(HERO_HEIGHT)
            .clip(shape)
            .border(1.dp, colors.edge, shape),
    ) {
        Backdrop(tile.background ?: tile.poster, Modifier.matchParentSize(), Scrims.heroSide, Scrims.heroFoot)
        Column(
            Modifier
                .align(Alignment.BottomStart)
                .padding(start = 56.dp, bottom = 48.dp)
                // Held on the scrim's dark side, as the frames hold the info column; DesignTest checks its
                // text over white art at this width.
                .widthIn(max = HERO_TEXT),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            Logo(tile.logo, tile.name, height = 120.dp, maxWidth = 520.dp, color = colors.text)
            RatingLine(TileCopy.ratings(tile.ratings), TileCopy.heroMeta(hero))
            hero.description?.let {
                // Measured last, so a two-line name in place of a logo takes its lines, never the buttons'.
                MqText(
                    it,
                    Modifier.weight(1f, fill = false),
                    role = TextRole.Callout,
                    color = colors.muted,
                    maxLines = 3,
                )
            }
            // The core's reason on Apple's explainability line, 16 over the buttons; never an eyebrow.
            MqText(hero.reason, role = TextRole.Caption, weight = FontWeight.SemiBold, maxLines = 1)
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
                verticalAlignment = Alignment.Top,
            ) {
                if (tile.playable()) {
                    PrimaryButton(
                        label = "Play",
                        icon = PlayFilled,
                        onClick = { onPlay(tile) },
                        modifier = Modifier.describe("Play ${tile.name}"),
                        focusRequester = playRequester,
                        onFocusChange = state.focus.watch(HERO_ROW, HERO_PLAY),
                    )
                }
                RoundButton(
                    icon = Icons.Info,
                    label = "Details",
                    onClick = { onOpen(tile) },
                    showLabel = true,
                    focusRequester = detailsRequester,
                    onFocusChange = state.focus.watch(HERO_ROW, HERO_DETAILS),
                )
            }
        }
    }
}

/** Continue Watching is a lane of 16:9 stills; catalogs keep the shape their first tile has. */
private fun shapeOf(row: HomeRow): TileShape =
    when {
        row.rowId == TileCopy.CONTINUE_ROW -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "landscape" -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "square" -> TileShape.Square
        else -> TileShape.Poster
    }

@Composable
private fun TvRow(
    row: HomeRow,
    /** The addon's name, when another row has the same title (TileCopy.source). */
    source: String?,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile, String) -> Unit)?,
    /** A failed row's Try again: that catalog only. */
    onRetry: (String) -> Unit,
    /**
     * The room a row's header takes over its tiles (its height, the gap under it and the tiles' lift room), for the
     * page's anchor: the anchor places a focused tile, and its header has to fit above.
     */
    onHeaderRoom: (Dp) -> Unit,
) {
    val inset = Marquee.safe.side
    val density = LocalDensity.current
    val gap = Marquee.space.xs
    val lift = Marquee.tiles.pad
    Column(verticalArrangement = Arrangement.spacedBy(gap)) {
        // A remote has no long press to discover: Continue Watching says how to remove a title.
        val hint = TileCopy.HOLD_TO_REMOVE.takeIf { onHold != null && TileCopy.removable(row.rowId) }
        RowHeader(
            row.title,
            source ?: hint,
            Modifier
                .padding(horizontal = inset)
                .onSizeChanged { if (it.height > 0) onHeaderRoom(with(density) { it.height.toDp() } + gap + lift) },
        )
        when (val notice = row.notice()) {
            is RowNotice.Waiting -> {
                SkeletonLane(shapeOf(row))
                val detail = notice.detail
                if (detail != null) {
                    MqText(
                        detail,
                        role = TextRole.Callout,
                        color = Marquee.colors.muted,
                        modifier = Modifier.padding(start = inset),
                    )
                }
            }

            is RowNotice.Explained -> {
                StateMessage(
                    title = notice.title,
                    body = notice.body,
                    icon = if (notice.failed) Icons.CircleAlert else null,
                    actionLabel = "Try again".takeIf { notice.failed },
                    onAction = { onRetry(row.rowId) }.takeIf { notice.failed },
                    modifier = Modifier.padding(start = inset),
                )
            }

            null -> {
                TileLane(row, state, onOpen, onHold?.takeIf { TileCopy.removable(row.rowId) })
            }
        }
    }
}

@Composable
private fun SkeletonLane(shape: TileShape) {
    val metrics = Marquee.tiles
    val size =
        if (shape ==
            TileShape.Poster
        ) {
            metrics.poster
        } else if (shape == TileShape.Landscape) {
            metrics.landscape
        } else {
            metrics.square
        }
    Row(
        Modifier.height(Marquee.tiles.laneHeight(shape)).padding(start = Marquee.safe.side, top = metrics.pad),
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
    ) {
        repeat(SKELETON_TILES) { Skeleton(Modifier.size(size.width, size.height)) }
    }
}

private const val SKELETON_TILES = 8

@Composable
private fun TileLane(
    row: HomeRow,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile, String) -> Unit)?,
) {
    val metrics = Marquee.tiles
    val shape = shapeOf(row)
    val keys = remember(row.tiles) { row.tileKeys() }
    // The lane's own leading edge: the page's anchor is lower by a header (TvHome).
    AnchoredScrolling(lead = tvContentTop(), trail = 0.dp) { Lane(row, state, onOpen, onHold, metrics, shape, keys) }
}

@Composable
private fun Lane(
    row: HomeRow,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile, String) -> Unit)?,
    metrics: TileMetrics,
    shape: TileShape,
    keys: List<String>,
) {
    LazyRow(
        state = state.rowState(row.rowId),
        modifier =
            Modifier
                .height(Marquee.tiles.laneHeight(shape))
                .focusProperties { enter = { state.focus.rowEntry(row.rowId) ?: FocusRequester.Default } }
                .focusGroup(),
        contentPadding = PaddingValues(horizontal = Marquee.safe.side),
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
    ) {
        itemsIndexed(row.tiles, key = { index, _ -> keys[index] }) { index, tile ->
            TvTile(row.rowId, keys[index], tile, shape, state, onOpen, onHold)
        }
    }
}

@Composable
private fun TvTile(
    rowId: String,
    key: String,
    tile: Tile,
    shape: TileShape,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile, String) -> Unit)?,
) {
    val requester = remember { FocusRequester() }
    DisposableEffect(key, requester) {
        state.focus.attach(key, requester)
        onDispose { state.focus.detach(key, requester) }
    }
    PosterTile(
        title = tile.name,
        imageUrl = if (shape == TileShape.Landscape) tile.background ?: tile.poster else tile.poster,
        onClick = { onOpen(tile) },
        modifier = Modifier.padding(top = Marquee.tiles.pad),
        shape = shape,
        progress = tile.progress,
        caption = TileCopy.caption(rowId, tile) ?: tile.releaseInfo,
        ratings = if (shape == TileShape.Poster) TileCopy.ratings(tile.ratings) else emptyList(),
        focusRequester = requester,
        onFocusChange = { if (it) state.focus.arrived(rowId, key) else state.focus.lost(key) },
        onLongClick = onHold?.let { { it(tile, key) } },
        longClickLabel = "More options".takeIf { onHold != null },
    )
}

/**
 * Puts focus on [wanted], or on the first tile when there is no [wanted] (first entry), and waits
 * for data when it is not there yet. Scrolls the lists so the node exists, asks for focus once
 * per frame, and checks focus actually landed on the intended key (a request can succeed on the
 * wrong node, and a fixed delay is flaky on slow TVs). Stops when the user moves first.
 */
internal suspend fun restoreFocus(
    home: HomeView,
    state: TvHomeState,
    wanted: String?,
    waitMs: Long = FOCUS_WAIT_MS,
) {
    val focus = state.focus
    // Counted before the first frame: a press during it is the viewer's move, not something to undo.
    val seen = focus.interactions
    val started = withFrameNanos { it }
    while (true) {
        val elapsed = (withFrameNanos { it } - started) / 1_000_000
        if (elapsed >= waitMs) return
        // A D-pad key went down since we started: the user took over.
        if (focus.interactions != seen) return
        val rows = home.rows.toList()
        val hasHero = home.hero != null
        val settled = rows.none { it.state == RowState.Loading || it.state == RowState.Slow }
        // A wanted tile that is gone for good (a finished Continue Watching item) gives way to
        // the first tile once the lanes have answered.
        val gone = wanted != null && settled && !exists(rows, hasHero, wanted)
        val key = if (wanted == null || gone) firstTarget(rows, home.hero, patient = elapsed < PATIENCE_MS) else wanted
        if (key == null || !reveal(rows, hasHero, state, key)) continue
        val requester = focus.requester(key) ?: continue
        try {
            requester.requestFocus()
        } catch (e: IllegalStateException) {
            continue // not attached yet: the next frame tries again
        }
        // A request can succeed on the wrong node: check that focus really is on the key now.
        if (focus.focusedKey == key && focus.hasFocus) {
            CoreLog.d(
                "focus on $key after ${(withFrameNanos { it } - started) / 1_000_000} ms, restore=${wanted != null}",
            )
            return
        }
    }
}

private fun exists(
    rows: List<HomeRow>,
    hasHero: Boolean,
    key: String,
): Boolean = if (key.startsWith("hero:")) hasHero else rows.any { key in it.tileKeys() }

/** The note under Continue Watching after Remove: its words, and Undo while it can be taken back. */
@Composable
private fun RemovalNote(
    words: String,
    canUndo: Boolean,
    undoFocus: FocusRequester,
    state: TvHomeState,
    onUndo: () -> Unit,
) {
    Row(
        Modifier.padding(start = Marquee.safe.side),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(
            words,
            role = TextRole.Callout,
            color = Marquee.colors.muted,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        )
        if (canUndo) {
            SecondaryButton(
                "Undo",
                onUndo,
                focusRequester = undoFocus,
                onFocusChange = state.focus.watch(null, UNDO_KEY),
            )
        }
    }
}

/** The focus key of the removal's Undo. */
private const val UNDO_KEY = "removal:undo"

/** How long Undo waits for its node to take the focus (it comes in with the note). */
private const val UNDO_FOCUS_FRAMES = 60

/**
 * Where focus goes when the tile [key] leaves its row (Remove from Continue Watching): the next tile, else
 * the one before it, else the first tile of the next row with tiles, else of the row before; null when
 * nothing is left.
 */
internal fun focusAfterRemove(
    rows: List<HomeRow>,
    key: String,
): String? {
    val at = rows.indexOfFirst { key in it.tileKeys() }
    if (at < 0) return null
    val keys = rows[at].tileKeys()
    val index = keys.indexOf(key)
    keys.getOrNull(index + 1)?.let { return it }
    keys.getOrNull(index - 1)?.let { return it }
    rows.drop(at + 1).firstNotNullOfOrNull { it.tileKeys().firstOrNull() }?.let { return it }
    return rows.take(at).asReversed().firstNotNullOfOrNull { it.tileKeys().firstOrNull() }
}

/**
 * What gets focus on first entry: the first tile of Continue Watching when that lane leads, else
 * the hero's Play when there is a hero (the web and Apple do the same), else the first tile of
 * the first lane. While the first lane is still loading and [patient], there is no target yet, so
 * focus never lands on a lower lane just because it answered first; lanes that came back empty or
 * failed are skipped.
 */
fun firstTarget(
    rows: List<HomeRow>,
    hero: Hero?,
    patient: Boolean,
): String? {
    if (rows.firstOrNull()?.rowId != TileCopy.CONTINUE_ROW) hero.focusKey()?.let { return it }
    for (row in rows) {
        row.tileKeys().firstOrNull()?.let { return it }
        if (patient && (row.state == RowState.Loading || row.state == RowState.Slow)) return null
    }
    return null
}

/**
 * The row where the first visit starts and where Back returns focus to: the row of [firstTarget],
 * so a lane that came back empty or failed is never named while focus lands below it. Focus on it
 * means the next Back leaves.
 */
internal fun startRow(
    rows: List<HomeRow>,
    hero: Hero?,
): String? {
    val key = firstTarget(rows, hero, patient = false) ?: return null
    return if (key.startsWith("hero:")) HERO_ROW else rows.firstOrNull { key in it.tileKeys() }?.rowId
}

private const val PATIENCE_MS = 3_000L

/** Where Home's one-time card goes: under Continue Watching, else first. */
internal fun cardSplit(rows: List<HomeRow>): Int = rows.indexOfFirst { it.rowId == TileCopy.CONTINUE_ROW } + 1

/** The page item of lane [rowIndex]: after the hero, and one lower under the card. */
internal fun pageIndex(
    rows: List<HomeRow>,
    rowIndex: Int,
    hasHero: Boolean,
    card: Boolean,
): Int = rowIndex + (if (hasHero) 1 else 0) + (if (card && rowIndex >= cardSplit(rows)) 1 else 0)

/** Scrolls the page and the key's lane until the key's node is composed. False when the key is not in the data (yet). */
private suspend fun reveal(
    rows: List<HomeRow>,
    hasHero: Boolean,
    state: TvHomeState,
    key: String,
): Boolean {
    if (key.startsWith("hero:")) {
        if (state.page.firstVisibleItemIndex != 0) state.page.scrollToItem(0)
        return true
    }
    for ((rowIndex, row) in rows.withIndex()) {
        val tileIndex = row.tileKeys().indexOf(key)
        if (tileIndex < 0) continue
        val itemIndex = pageIndex(rows, rowIndex, hasHero, state.cardShown)
        if (state.page.layoutInfo.visibleItemsInfo
                .none { it.index == itemIndex }
        ) {
            // The first lane shows with the hero above it, so its page position is the top.
            state.page.scrollToItem(if (rowIndex == 0) 0 else itemIndex)
        }
        val lane = state.rowState(row.rowId)
        if (lane.layoutInfo.visibleItemsInfo.none { it.index == tileIndex }) lane.scrollToItem(tileIndex)
        return true
    }
    return false
}

/** Back from anywhere on Home: focus where the first visit started (Continue Watching, else the hero, else the first lane). */
suspend fun focusFirstRow(
    home: HomeView,
    state: TvHomeState,
) {
    val key = firstTarget(home.rows.toList(), home.hero, patient = false) ?: return
    restoreFocus(home, state, key, waitMs = 1_500)
}
