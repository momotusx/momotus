package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/** The tile shapes of `design/tokens.json` (poster, landscape, square). */
enum class TileShape { Poster, Landscape, Square }

fun TileMetrics.sizeOf(shape: TileShape): TileSize =
    when (shape) {
        TileShape.Poster -> poster
        TileShape.Landscape -> landscape
        TileShape.Square -> square
    }

/** The row's picture height for a shape, which rows use to keep a fixed height in every state. */
fun TileMetrics.heightOf(shape: TileShape): Dp = sizeOf(shape).height

/**
 * The two lines under every tile, the name (`Callout`) and the secondary line (`Caption`), at the
 * current font scale. Tiles and their skeletons both use it, so a row never changes height.
 */
@Composable
fun captionHeight(): Dp {
    val type = Marquee.type
    return with(LocalDensity.current) { type.callout.toDp() * LINE + type.caption.toDp() * LINE } + 2.dp
}

/** The height of a row of [shape] tiles: room for the focus scale above and below, then the caption. */
@Composable
fun TileMetrics.laneHeight(shape: TileShape): Dp = pad * 2 + heightOf(shape) + Marquee.space.s + captionHeight()

/** `MqText`'s line height for text roles. */
private const val LINE = 1.4f

/**
 * A title in a row or grid (design/DECISION.md v2, Cards): the picture with a rating chip, then
 * the name and a secondary line under it, never over it. [caption] is that line: the core's own
 * words (Continue Watching, Library) or the year. Focus lifts the card, adds the ring and a white
 * edge, and turns the name accent. Everything is a plain value, so the design module knows nothing
 * of the core.
 */
@Composable
fun PosterTile(
    title: String,
    imageUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    shape: TileShape = TileShape.Poster,
    progress: Float? = null,
    caption: String? = null,
    /** The addon's IMDb rating as written ("8.4"); shown in a chip on the art. */
    rating: String? = null,
    focusRequester: FocusRequester? = null,
    forceFocus: Boolean = false,
    onFocusChange: ((Boolean) -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
) {
    val metrics = Marquee.tiles
    val colors = Marquee.colors
    val size = metrics.sizeOf(shape)
    val corner = Marquee.radius.m
    val lift = remember { mutableStateOf(forceFocus) }
    val focused = lift.value
    // What a screen reader says after the title: the caption, or the watched share.
    val state = caption ?: progress?.let { "${(it * 100).roundToInt()}% watched" }
    Column(
        // First in the chain: the focused tile draws above its neighbours in the row.
        modifier = Modifier.lifted(lift).then(modifier).width(size.width),
        // The caption starts a full step under the unscaled card, so a focused card's ring never covers it.
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        Box(
            Modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(corner, Tokens.FOCUS_SCALE_CARD, forced = forceFocus, lift = lift)
                .then(
                    if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier,
                ).clip(RoundedCornerShape(corner))
                .border(1.dp, if (focused) colors.text else Color.Transparent, RoundedCornerShape(corner))
                .semantics {
                    contentDescription = title
                    if (state != null) stateDescription = state
                }.focusClickable(onClick, Role.Button, onLongClick = onLongClick, longClickLabel = longClickLabel),
        ) {
            RemoteImage(imageUrl, size.width, size.height, fallbackText = title) {
                if (rating != null) RatingChip(rating, Modifier.align(Alignment.TopEnd))
                if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
            }
        }
        // Read as part of the tile above.
        Column(Modifier.clearAndSetSemantics { }) {
            MqText(
                title,
                role = TextRole.Callout,
                color = if (focused) colors.accent else colors.text,
                weight = FontWeight.Bold,
                maxLines = 1,
            )
            if (caption != null) {
                MqText(caption, role = TextRole.Caption, color = colors.dim, weight = FontWeight.Medium, maxLines = 1)
            }
        }
    }
}

/** A star and the rating on strong glass, at the top right of a poster. */
@Composable
private fun RatingChip(
    rating: String,
    modifier: Modifier,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    Row(
        modifier
            .padding(if (tv) 10.dp else 6.dp)
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(colors.glassStrong)
            .padding(horizontal = if (tv) 10.dp else 6.dp, vertical = if (tv) 4.dp else 2.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 6.dp else 3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(StarFilled, tint = colors.star, size = if (tv) 20.dp else 12.dp)
        MqText(rating, role = TextRole.Caption, weight = FontWeight.Bold, color = colors.text, maxLines = 1)
    }
}

/**
 * An episode on a card (design/DECISION.md v2, Episodes): a 16:9 still with the number at its
 * bottom left, a Watched chip and the watched line, then the title, the core's caption and two
 * lines of synopsis. The card is one control; [spoken] is what a screen reader says.
 */
@Composable
fun EpisodeCard(
    number: String,
    title: String,
    caption: String?,
    overview: String?,
    imageUrl: String?,
    watched: Boolean,
    progress: Float?,
    spoken: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
) {
    val metrics = Marquee.tiles
    val colors = Marquee.colors
    val size = metrics.landscape
    val corner = Marquee.radius.card
    val shape = RoundedCornerShape(corner)
    val lift = remember { mutableStateOf(false) }
    Column(
        Modifier
            .lifted(lift)
            .then(modifier)
            .width(size.width)
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(corner, Tokens.FOCUS_SCALE_CARD, lift = lift)
            .then(if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier)
            .clip(shape)
            .background(colors.raised)
            .border(1.dp, if (lift.value) colors.text else colors.edge, shape)
            .semantics(mergeDescendants = true) { contentDescription = spoken }
            .focusClickable(onClick, Role.Button, onLongClick = onLongClick, longClickLabel = longClickLabel),
    ) {
        RemoteImage(imageUrl, size.width, size.height, fallbackText = number) {
            if (watched) WatchedChip(Modifier.align(Alignment.TopStart))
            MqText(
                number,
                modifier =
                    Modifier
                        .align(Alignment.BottomStart)
                        .padding(10.dp)
                        .clip(RoundedCornerShape(Marquee.radius.pill))
                        .background(colors.bg)
                        .padding(horizontal = 10.dp, vertical = 3.dp),
                role = TextRole.Caption,
                weight = FontWeight.Bold,
                maxLines = 1,
            )
            if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
        }
        Column(
            Modifier.padding(horizontal = EPISODE_PAD, vertical = EPISODE_PAD * 0.75f).clearAndSetSemantics { },
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            MqText(title, role = TextRole.Callout, weight = FontWeight.Bold, maxLines = 1)
            if (caption != null) MqText(caption, role = TextRole.Caption, color = colors.dim, maxLines = 1)
            MqText(overview.orEmpty(), role = TextRole.Caption, color = colors.muted, maxLines = 2, minLines = 2)
        }
    }
}

/** The height of a lane of [EpisodeCard]s: the focus room, the still and the four text lines. */
@Composable
fun TileMetrics.episodeLaneHeight(): Dp {
    val type = Marquee.type
    val text = with(LocalDensity.current) { type.callout.toDp() * LINE + type.caption.toDp() * LINE * 3 }
    return pad * 2 + landscape.height + EPISODE_PAD * 1.5f + text + 8.dp
}

@Composable
private fun WatchedChip(modifier: Modifier) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Row(
        modifier
            .padding(if (tv) 10.dp else 6.dp)
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(colors.glassStrong)
            .padding(horizontal = if (tv) 10.dp else 6.dp, vertical = if (tv) 4.dp else 2.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 6.dp else 3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Check, tint = colors.text, size = if (tv) 18.dp else 12.dp)
        MqText("Watched", role = TextRole.Caption, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

private val EPISODE_PAD = 16.dp
