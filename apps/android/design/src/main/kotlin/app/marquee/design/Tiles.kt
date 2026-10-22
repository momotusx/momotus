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
import androidx.compose.ui.graphics.Shape
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

/**
 * The height of a row of [shape] tiles: room for the focus scale above and below, then the caption's room from the
 * tokens (the name and its line, [PosterTile]), which grows only when a larger font needs more. A [captioned] false
 * row is the art alone (the phone's Home posters).
 */
@Composable
fun TileMetrics.laneHeight(
    shape: TileShape,
    captioned: Boolean = true,
): Dp {
    if (!captioned) return pad * 2 + heightOf(shape)
    val room = if (shape == TileShape.Poster) captionPoster else caption
    // ponytail: the token at the default font size; a larger one takes what its two lines measure.
    val text = if (LocalDensity.current.fontScale > 1f) Marquee.space.s + captionHeight() else 0.dp
    return pad * 2 + heightOf(shape) + maxOf(room, text)
}

/** `MqText`'s line height for text roles. */
private const val LINE = 1.4f

// ponytail: the phone's caption room (tile.phone.caption, 46) less its two lines (34 measured); a token if web and
// Apple ever need the gap itself.
private val PHONE_CAPTION_GAP = 12.dp

/**
 * A title in a row or grid (design/DECISION.md, Shelves and cards): the picture, then the name and its line
 * under it, never over it. [caption] is the core's own words (Continue Watching, Library) or the year. The scores
 * are in a poster's cover itself (RPDB's band), so its caption is the name and that line; a 16:9 card's art is a
 * backdrop, so its scores follow the line ([RatingsRow]). Always two lines, so a row's captions align; a screen
 * reader hears the scores either way. The name at 600, the line at 64 percent, neither
 * changing on focus. [captioned] false is the art alone (the phone's Home posters). The art carries the 25 percent inner keyline (design/DECISION.md, Lines). Focus
 * lifts the card (on TV with its shadow and the sheen on the art, on a phone with the key ring).
 * Everything is a plain value, so the design module knows nothing of the core.
 */
@Composable
fun PosterTile(
    title: String,
    imageUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    shape: TileShape = TileShape.Poster,
    /** Shown when [imageUrl] cannot load. */
    fallbackImageUrl: String? = null,
    progress: Float? = null,
    caption: String? = null,
    /** The scores as plain text ("IMDb 8.4", "RT 87%"), read out after the title; the cover shows them. */
    ratings: List<String> = emptyList(),
    /** False leaves the caption out: the art alone. */
    captioned: Boolean = true,
    /** Where the crop keeps the picture: a poster in a 16:9 card keeps its upper part. */
    artAlignment: Alignment = Alignment.Center,
    /** A Watched chip at the top left of the art, as on episode cards. */
    watched: Boolean = false,
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
    // What a screen reader says after the title: the caption, or the watched share; the scores; and Watched.
    val state =
        (
            listOfNotNull(caption ?: progress?.let { "${(it * 100).roundToInt()}% watched" }) +
                ratings.map(::spokenScore) +
                listOfNotNull("Watched".takeIf { watched })
        ).joinToString(", ").ifEmpty { null }
    Column(
        // First in the chain: the focused tile draws above its neighbours in the row.
        modifier = Modifier.lifted(lift).then(modifier).width(size.width),
        // The caption starts a full step under the unscaled card on TV, so a focused card's lift rarely covers it; on a
        // phone 12 dp, so its two lines end inside the tokens' caption room.
        verticalArrangement = Arrangement.spacedBy(if (Marquee.form == Form.Tv) Marquee.space.s else PHONE_CAPTION_GAP),
    ) {
        Box(
            Modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(corner, Tokens.FOCUS_SCALE_ART, forced = forceFocus, lift = lift, art = true)
                .then(
                    if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier,
                ).clip(RoundedCornerShape(corner))
                .keyline(RoundedCornerShape(corner))
                .semantics {
                    contentDescription = title
                    if (state != null) stateDescription = state
                }.focusClickable(onClick, Role.Button, onLongClick = onLongClick, longClickLabel = longClickLabel),
        ) {
            RemoteImage(
                imageUrl,
                size.width,
                size.height,
                fallbackText = title,
                fallbackUrl = fallbackImageUrl,
                alignment = artAlignment,
            ) {
                if (watched) WatchedChip(Modifier.align(Alignment.TopStart))
                if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
            }
        }
        if (!captioned) return@Column
        // Read as part of the tile above.
        Column(Modifier.clearAndSetSemantics { }) {
            MqText(title, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
            // Two lines whatever the title has, so every tile of a row ends level.
            val line: @Composable (String) -> Unit = {
                MqText(it, role = TextRole.Caption, color = colors.dim, weight = FontWeight.Medium, maxLines = 1)
            }
            if (shape == TileShape.Poster) {
                line(caption.orEmpty())
            } else {
                // A 16:9 card's art is a backdrop, with no ratings band drawn in: the scores follow its line.
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    line(caption.orEmpty())
                    RatingsRow(ratings)
                }
            }
        }
    }
}

/**
 * What a screen reader says for a score: "IMDb 8.8", "Rotten Tomatoes 87 percent", never "R T". The Rotten
 * Tomatoes addon's mark is a word, not a number: "Rotten Tomatoes: Fresh".
 */
fun spokenScore(score: String): String {
    val rt = score.removePrefix("RT ").takeIf { it != score } ?: return score
    return if (PERCENT.matches(rt)) "Rotten Tomatoes ${rt.replace("%", " percent")}" else "Rotten Tomatoes: $rt"
}

private val PERCENT = Regex("\\d+%")

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
            .marqueeFocus(corner, Tokens.FOCUS_SCALE_ART, lift = lift, art = true)
            .then(if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier)
            .clip(shape)
            .background(colors.raised)
            .semantics(mergeDescendants = true) { contentDescription = spoken }
            .focusClickable(onClick, Role.Button, onLongClick = onLongClick, longClickLabel = longClickLabel),
    ) {
        // The keyline is the still's: the card's words sit on its fill.
        val still = RoundedCornerShape(topStart = corner, topEnd = corner)
        RemoteImage(imageUrl, size.width, size.height, fallbackText = number, modifier = Modifier.keyline(still)) {
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

/** The 25 percent inner keyline over art, so dark art never melts into the page: 1 point on TV, half on a phone. */
@Composable
private fun Modifier.keyline(shape: Shape): Modifier =
    border(if (Marquee.form == Form.Tv) 1.dp else 0.5.dp, Marquee.colors.lineStrong, shape)
