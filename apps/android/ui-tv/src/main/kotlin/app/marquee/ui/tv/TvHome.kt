@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.model.notice
import app.marquee.core.model.tileKeys
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Backdrop
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.Tokens
import app.marquee.design.describe
import app.marquee.design.heightOf
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.first

private const val HERO_KEY = "hero:open"
private val HERO_HEIGHT = 500.dp
private const val FOCUS_WAIT_MS = 6_000L

/**
 * Home on TV: a hero, then one lane per catalog. Focus starts on the first tile of the first lane
 * (Continue Watching when there is one), comes back to the exact tile after a title page, and
 * enters each lane on the tile that lane last had focused.
 */
@Composable
fun TvHome(
    home: HomeView,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
    modifier: Modifier = Modifier,
) {
    val safe = Marquee.safe
    val metrics = Marquee.tiles
    SideEffect {
        state.focus.firstRow = home.rows.firstOrNull()?.rowId
        state.focus.refresh()
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

    AnchoredScrolling(lead = ROW_LEAD, trail = 0.dp) {
        LazyColumn(
            state = state.page,
            modifier = modifier.fillMaxSize(),
            contentPadding = PaddingValues(bottom = safe.bottom),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            val hero = home.hero
            if (hero != null) item(key = "hero") { TvHero(hero, state, onOpen) }
            if (home.rows.isEmpty()) item(key = "state") { HomeState(home) }
            itemsIndexed(home.rows, key = { _, row -> row.rowId }) { _, row ->
                TvRow(row, state, onOpen)
            }
        }
    }
}

private val ROW_LEAD = 140.dp

@Composable
private fun HomeState(home: HomeView) {
    val inset = Marquee.tiles.inset
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
            StateMessage(
                title = "Nothing to show yet",
                body = "Home fills up as your add-ons answer.",
                modifier = Modifier.padding(start = inset, top = Marquee.safe.top),
            )
        }
    }
}

@Composable
private fun TvHero(
    hero: Hero,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
) {
    // Artwork is dark and busy in either theme: the hero always uses the dark scrim and light text.
    val scrim = Tokens.dark.bg
    val tile = hero.tile
    val requester = remember { FocusRequester() }
    DisposableEffect(requester) {
        state.focus.attach(HERO_KEY, requester)
        onDispose { state.focus.detach(HERO_KEY, requester) }
    }
    Box(Modifier.fillMaxWidth().height(HERO_HEIGHT)) {
        Backdrop(tile.background ?: tile.poster, Modifier.matchParentSize())
        // Scrims keep the title readable on any artwork and melt the picture into the rows below.
        Box(
            Modifier
                .matchParentSize()
                .background(
                    Brush.horizontalGradient(
                        0f to scrim,
                        0.55f to scrim.copy(alpha = 0.88f),
                        0.9f to scrim.copy(alpha = 0f),
                    ),
                ),
        )
        Box(
            Modifier
                .matchParentSize()
                .background(Brush.verticalGradient(0.55f to Color.Transparent, 1f to Marquee.colors.bg)),
        )
        Column(
            Modifier
                .align(Alignment.BottomStart)
                .padding(start = Marquee.tiles.inset, bottom = Marquee.space.l)
                .widthIn(max = 820.dp),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            MqText(hero.reason, role = TextRole.Callout, color = Tokens.dark.text, maxLines = 2)
            MqText(tile.name, role = TextRole.Hero, color = Tokens.dark.text, maxLines = 2)
            PrimaryButton(
                label = "Open",
                onClick = { onOpen(tile) },
                modifier = Modifier.describe("Open ${tile.name}"),
                focusRequester = requester,
                onFocusChange = { if (it) state.focus.arrived(null, HERO_KEY) else state.focus.lost(HERO_KEY) },
            )
        }
    }
}

private fun shapeOf(row: HomeRow): TileShape =
    when (row.tiles.firstOrNull()?.posterShape) {
        "landscape" -> TileShape.Landscape
        "square" -> TileShape.Square
        else -> TileShape.Poster
    }

private fun captionOf(tile: Tile): String? =
    if (tile.progress == null) {
        null
    } else {
        tile.videoId?.let(Format::parseVideo)?.let { Format.episodeLabel(it.season, it.episode) }
    }

@Composable
private fun TvRow(
    row: HomeRow,
    state: TvHomeState,
    onOpen: (Tile) -> Unit,
) {
    val inset = Marquee.tiles.inset
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        MqText(row.title, role = TextRole.Title3, modifier = Modifier.padding(start = inset), maxLines = 1)
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
                TileLane(row, state, onOpen)
            }
        }
    }
}

/** The lane's height depends only on its tile shape and on whether tiles carry captions, never on focus. */
@Composable
private fun laneHeight(
    shape: TileShape,
    captioned: Boolean,
) = Marquee.tiles.pad * 2 + Marquee.tiles.heightOf(shape) + (if (captioned) Marquee.tiles.caption else 0.dp)

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
        Modifier.height(laneHeight(shape, false)).padding(start = metrics.inset, top = metrics.pad),
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
) {
    val metrics = Marquee.tiles
    val shape = shapeOf(row)
    val keys = remember(row.tiles) { row.tileKeys() }
    val captioned = remember(row.tiles) { row.tiles.any { captionOf(it) != null } }
    LazyRow(
        state = state.rowState(row.rowId),
        modifier =
            Modifier
                .height(laneHeight(shape, captioned))
                .focusProperties { enter = { state.focus.rowEntry(row.rowId) ?: FocusRequester.Default } }
                .focusGroup(),
        contentPadding = PaddingValues(horizontal = metrics.inset),
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
    ) {
        itemsIndexed(row.tiles, key = { index, _ -> keys[index] }) { index, tile ->
            TvTile(row.rowId, keys[index], tile, shape, state, onOpen)
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
        caption = captionOf(tile),
        focusRequester = requester,
        onFocusChange = { if (it) state.focus.arrived(rowId, key) else state.focus.lost(key) },
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
    val started = withFrameNanos { it }
    val seen = focus.interactions
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
        val key = if (wanted == null || gone) firstTarget(rows, patient = elapsed < PATIENCE_MS) else wanted
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
): Boolean = if (key == HERO_KEY) hasHero else rows.any { key in it.tileKeys() }

/**
 * The tile that gets focus on first entry: the first tile of the first lane. While that lane is
 * still loading and [patient], there is no target yet, so focus never lands on a lower lane just
 * because it answered first; lanes that came back empty or failed are skipped.
 */
fun firstTarget(
    rows: List<HomeRow>,
    patient: Boolean,
): String? {
    for (row in rows) {
        row.tileKeys().firstOrNull()?.let { return it }
        if (patient && (row.state == RowState.Loading || row.state == RowState.Slow)) return null
    }
    return null
}

private const val PATIENCE_MS = 3_000L

/** Scrolls the page and the key's lane until the key's node is composed. False when the key is not in the data (yet). */
private suspend fun reveal(
    rows: List<HomeRow>,
    hasHero: Boolean,
    state: TvHomeState,
    key: String,
): Boolean {
    if (key == HERO_KEY) {
        if (state.page.firstVisibleItemIndex != 0) state.page.scrollToItem(0)
        return true
    }
    for ((rowIndex, row) in rows.withIndex()) {
        val tileIndex = row.tileKeys().indexOf(key)
        if (tileIndex < 0) continue
        val itemIndex = rowIndex + (if (hasHero) 1 else 0)
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

/** Back from anywhere on Home: focus the first tile of the first lane. */
suspend fun focusFirstRow(
    home: HomeView,
    state: TvHomeState,
) {
    val key = firstTarget(home.rows.toList(), patient = false) ?: return
    restoreFocus(home, state, key, waitMs = 1_500)
}
