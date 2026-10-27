package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.testTag
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
 * are in a poster's cover itself (RPDB's band) and nowhere in a caption, which is the name and that line, always two
 * lines so a row's captions align; a screen reader still hears them. The name at 600, the line at 64 percent, neither
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
    /** The score as plain text ("IMDb 8.4"), read out after the title; the cover shows it. */
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
    /** A Top 10 tile's place, drawn over the art's top left (plans/appletv-polish.md 4.3). */
    rank: Int? = null,
    /** A Top 10 tile's genre, one line under the art when the tile has no caption. */
    genre: String? = null,
    /** A small label on the art's top corner: the value a Discover list is ordered by ("IMDb 8.4", "2024"). */
    cornerLabel: String? = null,
    /** False: shown only, never focused or pressed (Discover's previous list, dimmed while the next loads). */
    enabled: Boolean = true,
) {
    val metrics = Marquee.tiles
    val colors = Marquee.colors
    val size = metrics.sizeOf(shape)
    val corner = Marquee.radius.m
    val lift = remember { mutableStateOf(forceFocus) }
    // What a screen reader says after the title: the caption, or the watched share; the scores; and Watched.
    val state =
        (
            listOfNotNull(rank?.let { "Number $it" }, genre) +
                listOfNotNull(caption ?: progress?.let { "${(it * 100).roundToInt()}% watched" }) +
                ratings +
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
                    // The corner's value is said with the title ("Dune: Part Two, IMDb 8.4").
                    contentDescription = listOfNotNull(title, cornerLabel).joinToString(", ")
                    if (state != null) stateDescription = state
                }.focusClickable(
                    onClick,
                    Role.Button,
                    enabled = enabled,
                    onLongClick = onLongClick,
                    longClickLabel = longClickLabel,
                ),
        ) {
            RemoteImage(
                imageUrl,
                size.width,
                size.height,
                fallbackText = title,
                fallbackUrl = fallbackImageUrl,
                alignment = artAlignment,
            ) {
                if (rank != null) RankNumeral(rank, Modifier.align(Alignment.TopStart))
                if (watched) WatchedChip(Modifier.align(Alignment.TopStart))
                // Bottom right, just above RPDB's ratings band (about a tenth of the cover) when the cover has one.
                // ponytail: an RPDB cover that fails shows the addon's poster, and the label then sits a tenth above
                // its foot; knowing which loaded means threading the image state up here.
                if (cornerLabel != null) {
                    val band = if (fallbackImageUrl != null) size.height * BAND_SHARE else 0.dp
                    CornerLabel(cornerLabel, Modifier.align(Alignment.BottomEnd).padding(bottom = band))
                }
                if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
            }
        }
        if (!captioned) {
            // A Top 10 poster with no caption keeps its genre, one quiet line (the owner's group C, C2 until picked).
            if (genre != null) {
                MqText(
                    genre,
                    Modifier.clearAndSetSemantics {
                        testTag = RANK_GENRE_TAG
                    },
                    role = TextRole.Caption,
                    color = colors.dim,
                    maxLines = 1,
                )
            }
            return@Column
        }
        // Read as part of the tile above.
        Column(Modifier.clearAndSetSemantics { }) {
            MqText(title, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
            // Two lines whatever the title has, so every tile of a row ends level.
            val line: @Composable (String) -> Unit = {
                MqText(it, role = TextRole.Caption, color = colors.dim, weight = FontWeight.Medium, maxLines = 1)
            }
            line(caption ?: genre.orEmpty())
        }
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

/** The share of an RPDB cover its ratings band takes at the bottom (84 of 859 px), plus a little air. */
private const val BAND_SHARE = 0.105f

/** The value a Discover list is ordered by, compact, at the art's bottom corner. */
@Composable
private fun CornerLabel(
    text: String,
    modifier: Modifier,
) {
    val tv = Marquee.form == Form.Tv
    MqText(
        text,
        modifier
            // Said with the title by the tile, not a second time.
            .clearAndSetSemantics { }
            .padding(if (tv) 8.dp else 4.dp)
            .clip(RoundedCornerShape(if (tv) 8.dp else 6.dp))
            .background(Marquee.colors.glassStrong)
            .padding(horizontal = if (tv) 8.dp else 5.dp, vertical = if (tv) 2.dp else 1.dp),
        role = TextRole.Caption,
        weight = FontWeight.SemiBold,
        maxLines = 1,
    )
}

private val EPISODE_PAD = 16.dp

/** The 25 percent inner keyline over art, so dark art never melts into the page: 1 point on TV, half on a phone. */
@Composable
private fun Modifier.keyline(shape: Shape): Modifier =
    border(if (Marquee.form == Form.Tv) 1.dp else 0.5.dp, Marquee.colors.lineStrong, shape)

/** A Top 10 tile's place: white over a soft shade in the art's corner; phone 27, TV 57, bold. */
@Composable
private fun RankNumeral(
    rank: Int,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    Box(
        modifier
            .size(if (tv) 160.dp else 72.dp)
            .background(
                Brush.radialGradient(
                    0f to Color.Black.copy(alpha = 0.45f),
                    0.45f to Color.Transparent,
                    center = Offset.Zero,
                    radius = if (tv) 300f else 140f,
                ),
            ),
    ) {
        MqText(
            rank.toString(),
            Modifier.padding(start = if (tv) 16.dp else 9.dp, top = if (tv) 6.dp else 3.dp),
            role = TextRole.Title1,
            weight = FontWeight.Bold,
            color = Color.White,
            maxLines = 1,
        )
    }
}

/** A Top 10 tile's genre line, for tests of where it sits. */
const val RANK_GENRE_TAG = "rank-genre"

/**
 * A streaming service on Discover's rail (plans/discover-v2.md 6.6, the owner, 2026-10-26): its logo on a
 * rounded square, as the Apple TV app and JustWatch draw them, with [name] as the words while the logo loads, when
 * there is none ([logoUrl] null: no TMDB key, or TMDB has none), and always as what a screen reader says. The chosen
 * one has a white ring; focus lifts it as art.
 */
@Composable
fun ServiceTile(
    name: String,
    logoUrl: String?,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
) {
    val side = if (Marquee.form == Form.Tv) 84.dp else 52.dp
    val corner = Marquee.radius.m
    val lift = remember { mutableStateOf(false) }
    val ring = 3.dp
    Box(
        Modifier
            .lifted(lift)
            .then(modifier)
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(corner + ring, Tokens.FOCUS_SCALE_ART, lift = lift, art = true)
            .then(if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier)
            .border(ring, if (selected) Marquee.colors.text else Color.Transparent, RoundedCornerShape(corner + ring))
            .padding(ring + 2.dp)
            .clip(RoundedCornerShape(corner))
            .semantics {
                contentDescription = name
                this.selected = selected
            }.focusClickable(onClick, Role.RadioButton),
    ) {
        var failed by remember(logoUrl) { mutableStateOf(false) }
        if (logoUrl != null && !failed) {
            RemoteImage(logoUrl, side, side, fallbackText = name, onFailed = { failed = true })
        } else {
            // No logo, or it did not load: the name on one line, the tile as wide as it (plans/discover-v2.md 6.6).
            Box(
                Modifier
                    .height(side)
                    .widthIn(min = side)
                    .background(Marquee.colors.surface)
                    .padding(horizontal = Marquee.space.s),
                contentAlignment = Alignment.Center,
            ) { MqText(name, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1, softWrap = false) }
        }
    }
}
