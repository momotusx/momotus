package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.unit.Constraints
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
    // In a row a TV focus turned white: the switch draws in dark ink.
    inverted: Boolean = false,
): SwitchColors =
    when {
        on && inverted -> {
            SwitchColors(track = colors.onPrimary, edge = Color.Transparent, knob = colors.primary)
        }

        on -> {
            SwitchColors(track = colors.primary, edge = Color.Transparent, knob = colors.onPrimary)
        }

        inverted -> {
            SwitchColors(
                track = Color.Transparent,
                edge = colors.onPrimary.copy(alpha = 0.64f),
                knob = colors.onPrimary.copy(alpha = 0.64f),
            )
        }

        else -> {
            SwitchColors(track = Color.Transparent, edge = colors.control, knob = colors.muted)
        }
    }

/** A switch as it looks; the row around it takes the focus and the toggle. */
@Composable
fun MqSwitch(
    on: Boolean,
    modifier: Modifier = Modifier,
    inverted: Boolean = false,
) {
    val tv = Marquee.form == Form.Tv
    val height = if (tv) 34.dp else 26.dp
    val knob = if (tv) 20.dp else 16.dp
    val colors = switchColors(on, Marquee.colors, inverted)
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
 * A row that is one switch: [content] (a mark, a name, a line) and the switch at its end. One focus
 * stop on TV, where the state is also said in a word; TalkBack reads the row's words with the switch
 * role and its state. [modifier] draws the row (focus, fill); [padding] sits inside the touch area.
 * On TV the focused row turns white with dark words ([focusPalette]), its switch too.
 *
 * [shape] rounds the row's fill, press veil and focus, which then reach the xs space past the words on each side:
 * the words keep the column of the card they sit in, and the rounded corners never cut a letter (a clip flush with
 * the words took the top of Torrentio's T and the r of raw.githubusercontent.com, Pixel walk 2026-10-30 F2).
 */
@Composable
fun SwitchRow(
    on: Boolean,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
    padding: PaddingValues = PaddingValues(),
    shape: Shape? = null,
    content: @Composable RowScope.() -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val colors = focusPalette(focused)
    val bleed = if (shape != null) Marquee.space.xs else 0.dp
    Row(
        Modifier
            .bleed(bleed)
            .then(modifier)
            .then(if (shape != null) Modifier.clip(shape) else Modifier)
            .onFocusChanged { focused = it.isFocused }
            .then(if (tv && focused) Modifier.background(colors.surface) else Modifier)
            .focusToggleable(on, onToggle)
            .padding(padding)
            .padding(horizontal = bleed),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FocusInk(colors) {
            content()
            MqSwitch(on, inverted = tv && focused)
            // As wide as the longer word, so a list's tracks line up.
            if (tv) {
                Box(Modifier.widthIn(min = 40.dp)) {
                    MqText(
                        if (on) "On" else "Off",
                        role = TextRole.Callout,
                        color = if (on) colors.text else colors.muted,
                    )
                }
            }
        }
    }
}

/** Lays the node out [x] wider on each side than its parent gives; to the parent it keeps the parent's width. */
private fun Modifier.bleed(x: Dp): Modifier =
    if (x == 0.dp) {
        this
    } else {
        layout { measurable, constraints ->
            val px = x.roundToPx()
            val wide =
                constraints.copy(
                    minWidth = constraints.minWidth + px * 2,
                    maxWidth = if (constraints.hasBoundedWidth) constraints.maxWidth + px * 2 else Constraints.Infinity,
                )
            val placeable = measurable.measure(wide)
            layout(placeable.width - px * 2, placeable.height) { placeable.place(-px, 0) }
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
