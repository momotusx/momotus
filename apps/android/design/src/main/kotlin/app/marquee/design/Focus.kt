package app.marquee.design

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.selection.selectable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import kotlinx.coroutines.flow.collectLatest
import kotlin.math.pow

/**
 * The focus look from design/DECISION.md, drawn the way the web's `--focus-shadow` is: a small
 * scale, a background-colored gap, a 4 px accent ring four points off the edge, and outside it a
 * dark halo that keeps 3:1 contrast on any artwork. The halo is 14 px and soft on the dark theme
 * (a few stacked rounded rects stand in for the blur) and a crisp 3 px line on the light one.
 * Never color alone: the scale moves too.
 *
 * Cost on weak GPUs: no blur, a handful of rounded rects only while focused. The focus flag and
 * the animation are read inside the graphics layer and the draw lambda, so a focus change redraws
 * this node and never recomposes the caller. Content that scales or draws outside its bounds
 * needs room around it (`Tokens` tile `pad`), because parents clip. A new focus target cancels
 * the fade of the old one, so a held D-pad leaves no trail.
 *
 * Place this before `clickable` or `focusable` in the chain: it observes the focus target after it.
 *
 * @param forced shows the focused look without focus (design gallery, screenshots).
 * @param lift when a component wraps the focused node (a tile with a caption), it lifts its own
 * root above its siblings with [lifted] and passes the flag here; without it this node lifts itself.
 */
@Composable
fun Modifier.marqueeFocus(
    radius: Dp,
    scale: Float = Tokens.FOCUS_SCALE_CARD,
    forced: Boolean = false,
    lift: MutableState<Boolean>? = null,
): Modifier {
    var focused by remember { mutableStateOf(false) }
    val progress = remember { Animatable(if (forced) 1f else 0f) }
    val raised = lift ?: remember { mutableStateOf(forced) }
    val millis = Marquee.focusMs
    LaunchedEffect(forced, millis) {
        snapshotFlow { focused || forced }.collectLatest { on ->
            if (on) raised.value = true
            progress.animateTo(if (on) 1f else 0f, tween(millis, easing = Tokens.ease))
            if (!on) raised.value = false
        }
    }
    val colors = Marquee.colors
    val halo = Marquee.focusHalo
    return this
        .onFocusChanged { focused = it.isFocused }
        .then(if (lift == null) Modifier.lifted(raised) else Modifier)
        .graphicsLayer {
            val s = 1f + (scale - 1f) * progress.value
            scaleX = s
            scaleY = s
        }.drawBehind {
            val p = progress.value
            if (p > 0f) {
                val corner = radius.toPx()
                val ring = Tokens.focusRing.toPx().coerceAtLeast(2f)
                val haloWidth = halo.toPx()
                val steps = if (haloWidth <= ring) 1 else HALO_STEPS
                // Stacked layers add up to the token's alpha where they all overlap.
                val layer = 1f - (1f - colors.halo.alpha * p).pow(1f / steps)
                for (i in 1..steps) {
                    val spread = ring * 2 + haloWidth * i / steps
                    drawRoundRect(
                        color = colors.halo.copy(alpha = layer),
                        topLeft = Offset(-spread, -spread),
                        size = Size(size.width + 2 * spread, size.height + 2 * spread),
                        cornerRadius = CornerRadius(corner + spread),
                    )
                }
                drawRoundRect(
                    color = colors.bg.copy(alpha = p),
                    topLeft = Offset(-ring, -ring),
                    size = Size(size.width + 2 * ring, size.height + 2 * ring),
                    cornerRadius = CornerRadius(corner + ring),
                )
                val centre = ring + ring / 2
                drawRoundRect(
                    color = colors.accent.copy(alpha = p),
                    topLeft = Offset(-centre, -centre),
                    size = Size(size.width + 2 * centre, size.height + 2 * centre),
                    cornerRadius = CornerRadius(corner + centre),
                    style = Stroke(width = ring),
                )
            }
        }
}

private const val HALO_STEPS = 3

/**
 * Draws this node above its siblings while [raised] is true, so its halo is not cut by the next
 * tile. Put it first in the chain: the draw order is decided by the node's outermost placement.
 * The flag is read while placing, so a change relayouts this node and recomposes nothing.
 */
fun Modifier.lifted(raised: MutableState<Boolean>): Modifier =
    layout { measurable, constraints ->
        val placeable = measurable.measure(constraints)
        layout(placeable.width, placeable.height) {
            placeable.place(0, 0, zIndex = if (raised.value) 1f else 0f)
        }
    }

/** Focusable and clickable (D-pad center and Enter click on TV) without the system ripple. */
@Composable
fun Modifier.focusClickable(
    onClick: () -> Unit,
    role: Role = Role.Button,
    enabled: Boolean = true,
): Modifier =
    clickable(
        interactionSource = remember { MutableInteractionSource() },
        indication = null,
        enabled = enabled,
        role = role,
        onClick = onClick,
    )

/** Like [focusClickable] for one choice of a group (filters): reports selected state to screen readers. */
@Composable
fun Modifier.focusSelectable(
    selected: Boolean,
    onClick: () -> Unit,
    role: Role = Role.RadioButton,
): Modifier =
    selectable(
        selected = selected,
        interactionSource = remember { MutableInteractionSource() },
        indication = null,
        role = role,
        onClick = onClick,
    )
