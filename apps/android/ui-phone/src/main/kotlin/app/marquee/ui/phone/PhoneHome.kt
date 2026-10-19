package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.marquee.core.TileCopy
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.Tile
import app.marquee.core.model.allFailed
import app.marquee.core.model.notice
import app.marquee.core.model.playable
import app.marquee.core.model.tileKeys
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Backdrop
import app.marquee.design.Eyebrow
import app.marquee.design.Icons
import app.marquee.design.Logo
import app.marquee.design.Marquee
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
import app.marquee.design.laneHeight
import app.marquee.design.windowDp

/** Home on a phone: a hero card, then one swipeable lane per catalog. */
@Composable
fun PhoneHome(
    home: HomeView,
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
) {
    val listState = rememberLazyListState()
    val hasHero = home.hero != null
    // The list keeps the first lane where it is when a hero arrives above it; someone who has not
    // scrolled yet should see the hero.
    LaunchedEffect(hasHero) {
        if (hasHero && listState.firstVisibleItemIndex <= 1 && listState.firstVisibleItemScrollOffset == 0) {
            listState.scrollToItem(0)
        }
    }
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars),
        contentPadding = PaddingValues(top = Marquee.safe.top, bottom = Marquee.space.l + LocalBarSpace.current),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        val hero = home.hero
        if (hero != null) item(key = "hero") { PhoneHero(hero, onOpen, onPlay) }
        if (home.rows.isEmpty()) item(key = "state") { HomeState(home, onSetup, onAddons, rowsHidden, onHomeRows) }
        if (hero == null && home.rows.toList().allFailed()) {
            item(key = "failed") { AllFailed(home, Modifier.padding(horizontal = Marquee.safe.side)) }
        } else {
            val rows = home.rows.toList()
            val split = rows.indexOfFirst { it.rowId == TileCopy.CONTINUE_ROW } + 1
            items(rows.subList(0, split), key = { it.rowId }) { row -> PhoneRow(row, onOpen) }
            if (card != null) item(key = "card") { card() }
            items(rows.subList(split, rows.size), key = { it.rowId }) { row -> PhoneRow(row, onOpen) }
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
    val side = Marquee.safe.side
    when (val status = home.status) {
        is ViewStatus.Failed -> {
            StateMessage(
                title = "Home could not load",
                body = status.reason,
                icon = Icons.CircleAlert,
                actionLabel = "Try again",
                onAction = home::retry,
                modifier = Modifier.padding(horizontal = side),
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
                    modifier = Modifier.padding(horizontal = side),
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
                modifier = Modifier.padding(horizontal = side),
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
private fun PhoneHero(
    hero: Hero,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
) {
    // An inset card (design/DECISION.md v2, Home hero). The scrim is 80% from a third of the way
    // down, where the text block begins, so white text reads at better than 8:1 on any artwork;
    // the accent eyebrow sits in its own chip.
    val scrim = Marquee.colors.bg
    val tile = hero.tile
    // At least the art's proportion, and taller when the text needs it (a small phone, large
    // fonts): the card grows and the page scrolls it, nothing is cut.
    BoxWithConstraints(Modifier.padding(horizontal = Marquee.safe.side).fillMaxWidth()) {
        val short =
            with(LocalDensity.current) {
                LocalWindowInfo.current.containerSize.height
                    .toDp()
            } < SHORT_WINDOW
        // Held to most of the window, so a landscape phone or a tablet still shows Play.
        val least = minOf(maxWidth / HERO_RATIO, windowDp().height * HERO_WINDOW)
        Box(
            Modifier
                .fillMaxWidth()
                .heightIn(min = least)
                .clip(RoundedCornerShape(Marquee.radius.hero))
                .border(1.dp, Marquee.colors.edge, RoundedCornerShape(Marquee.radius.hero))
                .semantics {
                    // The buttons on the card are hidden from screen readers, so Play is an action of the card.
                    if (tile.playable()) {
                        val play = CustomAccessibilityAction("Play ${tile.name}") { onPlay(tile).let { true } }
                        customActions = listOf(play)
                    }
                }.clickable(
                    role = Role.Button,
                    onClickLabel = "Details of ${tile.name}",
                    onClick = { onOpen(tile) },
                ),
        ) {
            Backdrop(tile.background ?: tile.poster, Modifier.matchParentSize())
            Box(
                Modifier
                    .matchParentSize()
                    .background(
                        Brush.verticalGradient(
                            0f to Color.Transparent,
                            0.3f to scrim.copy(alpha = 0.8f),
                            1f to scrim.copy(alpha = 0.96f),
                        ),
                    ),
            )
            Column(
                // The text starts no higher than a third down, where the scrim has darkened.
                Modifier
                    .align(Alignment.BottomStart)
                    .padding(top = least * if (short) 0.15f else 0.3f)
                    .padding(Marquee.space.m),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                Eyebrow(hero.reason, overArt = true)
                Logo(
                    tile.logo,
                    tile.name,
                    height = if (short) 44.dp else 64.dp,
                    maxWidth = 240.dp,
                    role = TextRole.Title1,
                    color = Marquee.colors.text,
                )
                RatingLine(tile.rating, TileCopy.heroMeta(hero))
                // A short window (a landscape phone) keeps Play above the bar: the synopsis waits on the
                // title page.
                if (!short) {
                    hero.description?.let {
                        MqText(it, role = TextRole.Caption, color = Marquee.colors.muted, maxLines = 3)
                    }
                }
                // The card is the control for screen readers (it opens the details); the buttons
                // repeat it for sighted touch.
                Row(
                    Modifier.padding(top = Marquee.space.xs).clearAndSetSemantics { },
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.Top,
                ) {
                    if (tile.playable()) PrimaryButton(label = "Play", icon = PlayFilled, onClick = { onPlay(tile) })
                    RoundButton(Icons.Info, "Details", { onOpen(tile) }, showLabel = true)
                }
            }
        }
    }
}

private const val HERO_RATIO = 0.95f

/** The most of the window's height a hero takes. */
private const val HERO_WINDOW = 0.6f

/** Below this window height the hero drops its synopsis and shrinks its logo. */
private val SHORT_WINDOW = 480.dp

/** Continue Watching is a lane of 16:9 stills; catalogs keep the shape their first tile has. */
private fun shapeOf(row: HomeRow): TileShape =
    when {
        row.rowId == TileCopy.CONTINUE_ROW -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "landscape" -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "square" -> TileShape.Square
        else -> TileShape.Poster
    }

@Composable
private fun PhoneRow(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
) {
    val side = Marquee.safe.side
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        RowHeader(row.title, TileCopy.source(row.rowId, row.source), Modifier.padding(horizontal = side))
        when (val notice = row.notice()) {
            is RowNotice.Waiting -> {
                SkeletonLane(shapeOf(row))
                val detail = notice.detail
                if (detail != null) {
                    MqText(
                        detail,
                        role = TextRole.Caption,
                        color = Marquee.colors.muted,
                        modifier = Modifier.padding(horizontal = side),
                    )
                }
            }

            is RowNotice.Explained -> {
                StateMessage(
                    title = notice.title,
                    body = notice.body,
                    icon = if (notice.failed) Icons.CircleAlert else null,
                    modifier = Modifier.padding(horizontal = side),
                )
            }

            null -> {
                TileLane(row, onOpen)
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

private const val SKELETON_TILES = 4

@Composable
private fun TileLane(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
) {
    val metrics = Marquee.tiles
    val shape = shapeOf(row)
    val keys = remember(row.tiles) { row.tileKeys() }
    LazyRow(
        modifier = Modifier.height(Marquee.tiles.laneHeight(shape)),
        contentPadding = PaddingValues(horizontal = Marquee.safe.side),
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
    ) {
        itemsIndexed(row.tiles, key = { index, _ -> keys[index] }) { _, tile ->
            PosterTile(
                title = tile.name,
                imageUrl = if (shape == TileShape.Landscape) tile.background ?: tile.poster else tile.poster,
                onClick = { onOpen(tile) },
                modifier = Modifier.padding(top = metrics.pad),
                shape = shape,
                progress = tile.progress,
                caption = TileCopy.caption(row.rowId, tile) ?: tile.releaseInfo,
                rating = tile.rating.takeIf { shape == TileShape.Poster },
            )
        }
    }
}
