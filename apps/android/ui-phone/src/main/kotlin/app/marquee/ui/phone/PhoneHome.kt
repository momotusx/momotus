package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
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
import app.marquee.design.RatingsRow
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.Scrims
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.describe
import app.marquee.design.laneHeight
import app.marquee.design.windowDp

/**
 * Home on a phone, as the iPhone TV app (design/DECISION.md, Home and Shelves): the 390 by 680 hero full bleed
 * under the status bar, then one swipeable lane per catalog: Continue Watching as 16:9 cards with their line,
 * the catalogs as bare 99 by 149 posters.
 */
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
    /** "Remove from Continue Watching", from a press and hold on one of its tiles; null offers no menu. */
    onRemove: ((Tile) -> Unit)? = null,
    /** The title of a removal waiting for its Undo: left out of Continue Watching meanwhile. */
    removing: String? = null,
    /** The removal's note (Undo, or why it failed), under Continue Watching. */
    notice: (@Composable () -> Unit)? = null,
) {
    var menuFor by remember { mutableStateOf<Tile?>(null) }
    menuFor?.let { tile ->
        ActionMenu(
            title = tile.name,
            actions = listOf(MenuAction(TileCopy.REMOVE_CONTINUE, Icons.X) { onRemove?.invoke(tile) }),
            onDismiss = { menuFor = null },
        )
    }
    val hold: ((Tile) -> Unit)? = onRemove?.let { { tile -> menuFor = tile } }
    val listState = rememberLazyListState()
    val hasHero = home.hero != null
    // The list keeps the first lane where it is when a hero arrives above it; someone who has not
    // scrolled yet should see the hero.
    LaunchedEffect(hasHero) {
        if (hasHero && listState.firstVisibleItemIndex <= 1 && listState.firstVisibleItemScrollOffset == 0) {
            listState.scrollToItem(0)
        }
    }
    // The hero runs up under the status bar; without one the page starts below it.
    val top = if (hasHero) 0.dp else WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + Marquee.safe.top
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(top = top, bottom = Marquee.space.l + LocalBarSpace.current),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        val hero = home.hero
        if (hero != null) item(key = "hero") { PhoneHero(hero, onOpen, onPlay) }
        if (home.rows.isEmpty()) item(key = "state") { HomeState(home, onSetup, onAddons, rowsHidden, onHomeRows) }
        if (hero == null && home.rows.toList().allFailed()) {
            item(key = "failed") { AllFailed(home, Modifier.padding(horizontal = Marquee.safe.side)) }
        } else {
            val rows = home.rows.toList().withoutRemoved(removing)
            val split = rows.indexOfFirst { it.rowId == TileCopy.CONTINUE_ROW } + 1
            items(rows.subList(0, split), key = {
                it.rowId
            }) { row -> PhoneRow(row, onOpen, hold, home::retryRow) }
            if (notice != null) item(key = "notice") { notice() }
            if (card != null) item(key = "card") { card() }
            items(
                rows.subList(split, rows.size),
                key = { it.rowId },
            ) { row -> PhoneRow(row, onOpen, hold, home::retryRow) }
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

/**
 * The iPhone's hero (the home-phone frame): the art full bleed, 390 by 680 on a 390 wide phone, the large title
 * over it, and a centered column at its foot: the logo, the kind and genres with the ratings row, the core's
 * reason, then the 178 by 48 Play pill and the 48 round Details. A tap on the art opens the details too.
 */
@Composable
private fun PhoneHero(
    hero: Hero,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
) {
    val tile = hero.tile
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        // The frame's shape, held to most of a short window (a landscape phone) so Play still shows.
        val height = minOf(maxWidth * HERO_RATIO, windowDp().height * HERO_WINDOW)
        Box(
            Modifier
                .fillMaxWidth()
                .height(height)
                .testTag(PHONE_HERO_TAG)
                .semantics {
                    // The buttons on the hero are hidden from screen readers, so Play is an action of the hero, which
                    // itself opens the details.
                    if (tile.playable()) {
                        customActions =
                            listOf(CustomAccessibilityAction("Play ${tile.name}") { onPlay(tile).let { true } })
                    }
                }.clickable(role = Role.Button, onClickLabel = "Details of ${tile.name}") { onOpen(tile) },
        ) {
            Backdrop(
                tile.background ?: tile.poster,
                Modifier.matchParentSize(),
                foot = Scrims.phoneHomeFoot,
                top = Scrims.phoneHomeTop,
                alignment = if (tile.background == null) Alignment.TopCenter else Alignment.Center,
            )
            MqText(
                "Home",
                Modifier
                    .windowInsetsPadding(WindowInsets.statusBars)
                    .padding(start = 16.dp, top = 4.dp)
                    .semantics { heading() },
                role = TextRole.Hero,
                maxLines = 1,
            )
            Column(
                Modifier
                    .align(Alignment.BottomCenter)
                    .padding(start = Marquee.safe.side, end = Marquee.safe.side, bottom = HERO_FOOT),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Logo(
                    tile.logo,
                    tile.name,
                    height = 86.dp,
                    maxWidth = 300.dp,
                    role = TextRole.Title1,
                    color = Marquee.colors.text,
                )
                FlowRow(
                    Modifier.padding(top = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    itemVerticalAlignment = Alignment.CenterVertically,
                ) {
                    MqText(
                        TileCopy.heroGenres(hero),
                        role = TextRole.Callout,
                        weight = FontWeight.SemiBold,
                        maxLines = 1,
                    )
                    RatingsRow(TileCopy.ratings(tile.ratings), onArt = true)
                }
                // The core's reason on Apple's explainability line; never an eyebrow.
                if (hero.reason.isNotBlank()) {
                    MqText(
                        hero.reason,
                        Modifier.padding(top = 6.dp),
                        role = TextRole.Caption,
                        weight = FontWeight.SemiBold,
                        maxLines = 2,
                        textAlign = TextAlign.Center,
                    )
                }
                // The hero is the control for screen readers (Details, and Play as its action); the buttons repeat
                // it for sighted touch.
                Row(
                    Modifier.padding(top = 18.dp).clearAndSetSemantics { testTag = PHONE_HERO_BUTTONS },
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (tile.playable()) {
                        PrimaryButton(
                            label = "Play",
                            icon = PlayFilled,
                            onClick = { onPlay(tile) },
                            modifier = Modifier.width(178.dp).describe("Play ${tile.name}"),
                        )
                    }
                    RoundButton(Icons.Info, "Details", { onOpen(tile) })
                }
            }
        }
    }
}

/** The hero and its button row, for tests of where they sit. */
internal const val PHONE_HERO_TAG = "home-hero"
internal const val PHONE_HERO_BUTTONS = "home-hero-buttons"

/** The frame's 390 by 680. */
private const val HERO_RATIO = 680f / 390f

/** The most of the window's height a hero takes. */
private const val HERO_WINDOW = 0.85f

/** The buttons' foot over the hero's (the frame's 56; its carousel dots wait for a source). */
private val HERO_FOOT = 56.dp

/** Continue Watching is a lane of 16:9 stills; catalogs keep the shape their first tile has. */
private fun shapeOf(row: HomeRow): TileShape =
    when {
        row.rowId == TileCopy.CONTINUE_ROW -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "landscape" -> TileShape.Landscape
        row.tiles.firstOrNull()?.posterShape == "square" -> TileShape.Square
        else -> TileShape.Poster
    }

/** A catalog's posters are the art alone on the phone; Continue Watching keeps its name and line. */
private fun captioned(row: HomeRow): Boolean = TileCopy.isContinue(row.rowId)

@Composable
private fun PhoneRow(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
    hold: ((Tile) -> Unit)?,
    /** A failed row's Try again: that catalog only. */
    onRetry: (String) -> Unit,
) {
    val side = Marquee.safe.side
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        // The catalog's name and kind alone, never its addon (design/DECISION.md, Shelves).
        RowHeader(row.title, null, Modifier.padding(horizontal = side))
        when (val notice = row.notice()) {
            is RowNotice.Waiting -> {
                SkeletonLane(shapeOf(row), captioned(row))
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
                    actionLabel = "Try again".takeIf { notice.failed },
                    onAction = { onRetry(row.rowId) }.takeIf { notice.failed },
                    modifier = Modifier.padding(horizontal = side),
                )
            }

            null -> {
                TileLane(row, onOpen, hold?.takeIf { TileCopy.removable(row.rowId) })
            }
        }
    }
}

@Composable
private fun SkeletonLane(
    shape: TileShape,
    captioned: Boolean = true,
) {
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
        Modifier
            .height(
                Marquee.tiles.laneHeight(shape, captioned),
            ).padding(start = Marquee.safe.side, top = metrics.pad),
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
    onHold: ((Tile) -> Unit)?,
) {
    val metrics = Marquee.tiles
    val shape = shapeOf(row)
    val keys = remember(row.tiles) { row.tileKeys() }
    val captioned = captioned(row)
    LazyRow(
        modifier = Modifier.height(Marquee.tiles.laneHeight(shape, captioned)),
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
                // Continue Watching carries the core's line alone: no scores, no badges.
                captioned = captioned,
                artAlignment =
                    if (shape == TileShape.Landscape &&
                        tile.background == null
                    ) {
                        Alignment.TopCenter
                    } else {
                        Alignment.Center
                    },
                onLongClick = onHold?.let { { it(tile) } },
                longClickLabel = "More options".takeIf { onHold != null },
            )
        }
    }
}
