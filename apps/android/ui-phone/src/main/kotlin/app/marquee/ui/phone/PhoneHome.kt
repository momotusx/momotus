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
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
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
import app.marquee.design.heightOf

/** Home on a phone: a hero card, then one swipeable lane per catalog. */
@Composable
fun PhoneHome(
    home: HomeView,
    onOpen: (Tile) -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        state = rememberLazyListState(),
        modifier = modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars),
        contentPadding = PaddingValues(top = Marquee.safe.top, bottom = Marquee.space.l),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        val hero = home.hero
        if (hero != null) item(key = "hero") { PhoneHero(hero, onOpen) }
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
                modifier = Modifier.padding(horizontal = side),
            )
        }

        ViewStatus.Opening -> {
            SkeletonLane(TileShape.Poster)
        }

        ViewStatus.Open -> {
            StateMessage(
                title = "Nothing to show yet",
                body = "Home fills up as your add-ons answer.",
                modifier = Modifier.padding(horizontal = side),
            )
        }
    }
}

@Composable
private fun PhoneHero(
    hero: Hero,
    onOpen: (Tile) -> Unit,
) {
    val colors = Marquee.colors
    val tile = hero.tile
    Box(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .aspectRatio(HERO_RATIO)
            .clip(RoundedCornerShape(Marquee.radius.l))
            .clickable(role = Role.Button, onClick = { onOpen(tile) }),
    ) {
        Backdrop(tile.background ?: tile.poster, Modifier.matchParentSize())
        // Keeps the title readable on any artwork.
        Box(
            Modifier
                .matchParentSize()
                .background(Brush.verticalGradient(0.3f to Color.Transparent, 1f to colors.bg.copy(alpha = 0.92f))),
        )
        Column(
            Modifier.align(Alignment.BottomStart).padding(Marquee.space.m),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            MqText(hero.reason, role = TextRole.Caption, color = colors.muted, maxLines = 2)
            MqText(tile.name, role = TextRole.Title1, maxLines = 2)
            PrimaryButton(label = "Open", onClick = { onOpen(tile) })
        }
    }
}

private const val HERO_RATIO = 1.5f

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
private fun PhoneRow(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
) {
    val side = Marquee.safe.side
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        MqText(row.title, role = TextRole.Title3, modifier = Modifier.padding(horizontal = side), maxLines = 1)
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
    val captioned = remember(row.tiles) { row.tiles.any { captionOf(it) != null } }
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
                caption = captionOf(tile),
            )
        }
    }
}
