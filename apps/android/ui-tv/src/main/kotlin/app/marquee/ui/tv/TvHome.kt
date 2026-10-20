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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
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
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.ActionMenu
import app.marquee.design.Backdrop
import app.marquee.design.Eyebrow
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
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.Tokens
import app.marquee.design.describe
import app.marquee.design.laneHeight
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

private const val HERO_PLAY = "hero:play"
private const val HERO_DETAILS = "hero:details"
private const val HERO_ROW = "hero"
private val HERO_HEIGHT = 560.dp
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
    /** "Remove from Continue Watching", from a held select on one of its tiles; null offers no menu. */
    onRemove: ((Tile) -> Unit)? = null,
) {
    val safe = Marquee.safe
    val metrics = Marquee.tiles
    // The held tile and its key, while its menu is open.
    var menuFor by remember { mutableStateOf<Pair<Tile, String>?>(null) }
    val scope = rememberCoroutineScope()
    menuFor?.let { (tile, key) ->
        ActionMenu(
            title = tile.name,
            actions =
                listOf(
                    MenuAction(TileCopy.REMOVE_CONTINUE, Icons.X) {
                        // Focus goes on from the tile that leaves: the next one, the one before, or the next row.
                        val next = focusAfterRemove(home.rows.toList(), key)
                        onRemove?.invoke(tile)
                        if (next != null) scope.launch { restoreFocus(home, state, next) }
                    },
                ),
            onDismiss = { menuFor = null },
        )
    }
    val hold: ((Tile, String) -> Unit)? = onRemove?.let { { tile, key -> menuFor = tile to key } }
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

    // A focused row rests under the pill, never behind it.
    AnchoredScrolling(lead = tvContentTop(), trail = 0.dp) {
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
                val rows = home.rows.toList()
                val split = cardSplit(rows)
                state.cardShown = card != null
                items(rows.subList(0, split), key = { it.rowId }) { row -> TvRow(row, state, onOpen, hold) }
                if (card != null) item(key = "card") { card() }
                items(rows.subList(split, rows.size), key = { it.rowId }) { row -> TvRow(row, state, onOpen, hold) }
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
    val scrim = colors.bg
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
        Backdrop(tile.background ?: tile.poster, Modifier.matchParentSize())
        Box(
            Modifier
                .matchParentSize()
                .background(
                    Brush.horizontalGradient(
                        0f to scrim.copy(alpha = 0.94f),
                        0.28f to scrim.copy(alpha = 0.72f),
                        0.58f to scrim.copy(alpha = 0.18f),
                        0.72f to scrim.copy(alpha = 0f),
                    ),
                ),
        )
        Box(
            Modifier
                .matchParentSize()
                .background(
                    Brush.verticalGradient(
                        0.42f to Color.Transparent,
                        0.66f to scrim.copy(alpha = 0.3f),
                        1f to scrim.copy(alpha = 0.9f),
                    ),
                ),
        )
        Column(
            Modifier
                .align(Alignment.BottomStart)
                .padding(start = 56.dp, bottom = 48.dp)
                .widthIn(max = 860.dp),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            Eyebrow(hero.reason, overArt = true)
            Logo(tile.logo, tile.name, height = 120.dp, maxWidth = 520.dp, color = colors.text)
            RatingLine(tile.rating, TileCopy.heroMeta(hero))
            hero.description?.let {
                MqText(it, role = TextRole.Callout, color = colors.muted, maxLines = 3)
            }
            Row(
                Modifier.padding(top = Marquee.space.xs),
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
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile, String) -> Unit)?,
) {
    val inset = Marquee.safe.side
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        RowHeader(row.title, TileCopy.source(row.rowId, row.source), Modifier.padding(horizontal = inset))
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
        rating = tile.rating.takeIf { shape == TileShape.Poster },
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
