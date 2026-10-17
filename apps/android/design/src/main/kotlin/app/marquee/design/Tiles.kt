package app.marquee.design

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp

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
) {
    val metrics = Marquee.tiles
    val size = metrics.sizeOf(shape)
    val corner = Marquee.radius.m
    val description = if (caption != null) "$title, $caption" else title
    Column(
        modifier = modifier.width(size.width),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        Box(
            Modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(corner, Tokens.FOCUS_SCALE_CARD, forced = forceFocus)
                .clip(RoundedCornerShape(corner))
                .semantics { contentDescription = description }
                .focusClickable(onClick, Role.Button),
        ) {
            RemoteImage(imageUrl, size.width, size.height, fallbackText = title) {
                if (progress != null) ProgressBar(progress, Modifier.align(Alignment.BottomStart))
            }
        }
        if (caption != null) {
            MqText(
                caption,
                role = TextRole.Callout,
                color = Marquee.colors.muted,
                weight = FontWeight.Medium,
                maxLines = 1,
            )
        }
    }
}
