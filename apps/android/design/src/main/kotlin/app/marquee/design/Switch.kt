package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage

/** A switch's colors: its track, the track's edge and the knob. */
internal data class SwitchColors(
    val track: Color,
    val edge: Color,
    val knob: Color,
)

/**
 * How a switch draws (design/audit-web-2026-10-19.md, fourth pass, as the web and Apple): on is a
 * white track with a dark knob, off an empty track with the control edge. Never the accent: a list
 * of accent tracks would be a large fill, and the white stays small beside the one white action.
 */
internal fun switchColors(
    on: Boolean,
    colors: Palette,
): SwitchColors =
    if (on) {
        SwitchColors(track = colors.primary, edge = Color.Transparent, knob = colors.onPrimary)
    } else {
        SwitchColors(track = Color.Transparent, edge = colors.control, knob = colors.muted)
    }

/** A switch as it looks; the row around it takes the focus and the toggle. */
@Composable
fun MqSwitch(
    on: Boolean,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    val height = if (tv) 34.dp else 26.dp
    val knob = if (tv) 20.dp else 16.dp
    val colors = switchColors(on, Marquee.colors)
    val shape = RoundedCornerShape(height / 2)
    Box(
        modifier
            .size(if (tv) 60.dp else 44.dp, height)
            .clip(shape)
            .background(colors.track)
            .border(2.dp, colors.edge, shape),
    ) {
        Box(
            Modifier
                .align(if (on) Alignment.CenterEnd else Alignment.CenterStart)
                .padding(horizontal = (height - knob) / 2)
                .size(knob)
                .clip(CircleShape)
                .background(colors.knob),
        )
    }
}

/**
 * An addon's mark: its logo on a neutral tile, or the name's first letter when there is none or it
 * cannot load. Decorative: the row it leads names the addon.
 */
@Composable
fun MarkTile(
    url: String?,
    name: String,
    modifier: Modifier = Modifier,
    size: Dp = 56.dp,
) {
    var failed by remember(url) { mutableStateOf(url == null) }
    val shape = RoundedCornerShape(Marquee.radius.m)
    Box(
        modifier
            .size(size)
            .clip(shape)
            .background(Marquee.colors.raised)
            .border(1.dp, Marquee.colors.edge, shape),
        contentAlignment = Alignment.Center,
    ) {
        if (failed) {
            MqText(name.take(1).uppercase(), role = TextRole.Title3, color = Marquee.colors.muted)
        } else {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Fit,
                onError = { failed = true },
                modifier = Modifier.fillMaxSize().padding(size / 7),
            )
        }
    }
}
