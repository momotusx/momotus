package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/**
 * What a Continue Watching still shows on top of the picture: the title's logo (its name as text
 * when there is none) bottom left over a dark strip, and the watched bar on the bottom edge.
 */
data class UpNextArt(
    val logoUrl: String?,
)

@Composable
private fun BoxScope.UpNextOverlay(
    art: UpNextArt,
    name: String,
    progress: Float?,
) {
    val tv = Marquee.form == Form.Tv
    // The artwork is any photo: the strip is the dark theme color in both themes, with light text.
    val strip = Tokens.dark.bg
    val bar = if (tv) 6.dp else 4.dp
    Box(
        Modifier
            .align(Alignment.BottomStart)
            .fillMaxWidth()
            .fillMaxHeight(0.55f)
            .background(Brush.verticalGradient(0f to Color.Transparent, 1f to strip.copy(alpha = 0.88f))),
    )
    Logo(
        url = art.logoUrl,
        name = name,
        height = if (tv) 56.dp else 32.dp,
        maxWidth = if (tv) 240.dp else 130.dp,
        modifier = Modifier.align(Alignment.BottomStart).padding(start = 14.dp, bottom = bar + 12.dp),
        role = if (tv) TextRole.Headline else TextRole.Callout,
        color = Tokens.dark.text,
        decorative = true,
    )
    if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
}

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
 * A title in a row or grid: the picture with the focus look, a watched bar for items in
 * progress, and an optional caption such as "S1:E2, 40% watched". Everything is a plain value,
 * so the design module knows nothing of the core.
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
    focusRequester: FocusRequester? = null,
    forceFocus: Boolean = false,
    onFocusChange: ((Boolean) -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
    upNext: UpNextArt? = null,
) {
    val metrics = Marquee.tiles
    val size = metrics.sizeOf(shape)
    val corner = Marquee.radius.m
    val lift = remember { mutableStateOf(forceFocus) }
    // What a screen reader says after the title: the caption, or the watched share.
    val state = caption ?: progress?.let { "${(it * 100).roundToInt()}% watched" }
    Column(
        // First in the chain: the focused tile draws above its neighbours in the row.
        modifier = Modifier.lifted(lift).then(modifier).width(size.width),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        Box(
            Modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(corner, Tokens.FOCUS_SCALE_CARD, forced = forceFocus, lift = lift)
                .then(
                    if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier,
                ).clip(RoundedCornerShape(corner))
                .semantics {
                    contentDescription = title
                    if (state != null) stateDescription = state
                }.focusClickable(onClick, Role.Button, onLongClick = onLongClick, longClickLabel = longClickLabel),
        ) {
            RemoteImage(imageUrl, size.width, size.height, fallbackText = title) {
                if (upNext != null) {
                    UpNextOverlay(upNext, title, progress)
                } else if (progress != null) {
                    ProgressBar(progress, Modifier.align(Alignment.BottomStart))
                }
            }
        }
        if (caption != null) {
            MqText(
                caption,
                // Read as part of the tile above.
                modifier = Modifier.clearAndSetSemantics { },
                role = TextRole.Callout,
                color = Marquee.colors.muted,
                weight = FontWeight.Medium,
                maxLines = 2,
            )
        }
    }
}
