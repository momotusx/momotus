package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
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
import app.marquee.core.model.notice
import app.marquee.core.model.playable
import app.marquee.core.model.tileKeys
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Backdrop
import app.marquee.design.Icons
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.OverPicture
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.Tokens
import app.marquee.design.UpNextArt
import app.marquee.design.laneHeight

/** Home on a phone: a hero card, then one swipeable lane per catalog. */
@Composable
fun PhoneHome(
    home: HomeView,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
    modifier: Modifier = Modifier,
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
        contentPadding = PaddingValues(top = Marquee.safe.top, bottom = Marquee.space.l),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        val hero = home.hero
        if (hero != null) item(key = "hero") { PhoneHero(hero, onOpen, onPlay) }
        if (home.rows.isEmpty()) item(key = "state") { HomeState(home) }
        itemsIndexed(home.rows, key = { _, row -> row.rowId }) { _, row -> PhoneRow(row, onOpen) }
    }
}

@Composable
private fun HomeState(home: HomeView) {
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
            StateMessage(
                title = "Nothing to show yet",
                body = "Home fills up as your addons answer.",
                modifier = Modifier.padding(horizontal = side),
            )
        }
    }
}

@Composable
private fun PhoneHero(
    hero: Hero,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
) {
    // Artwork is dark and busy in either theme: the card always uses the dark scrim and light text.
    // The scrim is already 80% from a third of the way down, which is where the text block begins,
    // so the reason line over the brightest bokeh still reads at better than 8:1.
    val scrim = Tokens.dark.bg
    val tile = hero.tile
    Box(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .aspectRatio(HERO_RATIO)
            .clip(RoundedCornerShape(Marquee.radius.l))
            .semantics {
                // The buttons on the card are hidden from screen readers, so Play is an action of the card.
                if (tile.playable()) {
                    customActions = listOf(CustomAccessibilityAction("Play ${tile.name}") { onPlay(tile).let { true } })
                }
            }.clickable(role = Role.Button, onClickLabel = "Details of ${tile.name}", onClick = { onOpen(tile) }),
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
            Modifier.align(Alignment.BottomStart).padding(Marquee.space.m),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            Logo(
                tile.logo,
                tile.name,
                height = 64.dp,
                maxWidth = 240.dp,
                role = TextRole.Title1,
                color = Tokens.dark.text,
            )
            MqText(hero.reason, role = TextRole.Title3, color = Tokens.dark.text, maxLines = 2, italic = true)
            MqText(TileCopy.heroMeta(tile), role = TextRole.Caption, color = Tokens.dark.muted)
            // The card is the control for screen readers (it opens the details); the buttons
            // repeat it for sighted touch.
            OverPicture {
                Row(
                    Modifier.padding(top = Marquee.space.xs).clearAndSetSemantics { },
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    if (tile.playable()) PrimaryButton(label = "Play", icon = Icons.Play, onClick = { onPlay(tile) })
                    SecondaryButton(label = "Details", icon = Icons.Info, onClick = { onOpen(tile) })
                }
            }
        }
    }
}

private const val HERO_RATIO = 0.95f

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
        Row(
            Modifier.padding(horizontal = side),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.Bottom,
        ) {
            MqText(row.title, role = TextRole.Title3, modifier = Modifier.weight(1f, fill = false), maxLines = 1)
            MqText(
                TileCopy.source(row.rowId, row.source),
                role = TextRole.Caption,
                color = Marquee.colors.muted,
                modifier = Modifier.padding(bottom = 2.dp),
                maxLines = 1,
            )
        }
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

/** A lane's height depends only on its tile shape and on captions, so it never shifts under the thumb. */
@Composable
private fun laneHeight(
    shape: TileShape,
    captioned: Boolean,
) = Marquee.tiles.laneHeight(shape, captioned)

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
        Modifier.height(laneHeight(shape, false)).padding(start = Marquee.safe.side, top = metrics.pad),
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
    val captioned = remember(row.tiles) { row.tiles.any { TileCopy.caption(row.rowId, it) != null } }
    val wide = row.rowId == TileCopy.CONTINUE_ROW
    LazyRow(
        modifier = Modifier.height(laneHeight(shape, captioned)),
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
                caption = TileCopy.caption(row.rowId, tile),
                upNext = if (wide) UpNextArt(tile.logo) else null,
            )
        }
    }
}
