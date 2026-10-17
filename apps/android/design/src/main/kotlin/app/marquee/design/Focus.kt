package app.marquee.design

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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

/**
 * The focus look from design/DECISION.md: a small scale, a 4 px accent ring four points off the
 * edge, and a dark halo behind it that keeps 3:1 contrast on any artwork (the web draws the same
 * with an outline and a box-shadow). Never color alone: the scale moves too.
 *
 * Cost on weak GPUs: no blur, two rounded rects only while focused. The focus flag and the
 * animation are read inside the graphics layer and the draw lambda, so a focus change redraws
 * this node and never recomposes the caller. Content that scales or draws outside its bounds
 * needs room around it (`Tokens` tile `pad`), because parents clip.
 *
 * Place this before `clickable` or `focusable` in the chain: it observes the focus target after it.
 *
 * @param forced shows the focused look without focus (design gallery, screenshots).
 */
@Composable
fun Modifier.marqueeFocus(
    radius: Dp,
    scale: Float = Tokens.FOCUS_SCALE_CARD,
    forced: Boolean = false,
): Modifier {
    var focused by remember { mutableStateOf(false) }
    val progress = remember { Animatable(if (forced) 1f else 0f) }
    val millis = Marquee.focusMs
    LaunchedEffect(forced, millis) {
        snapshotFlow { focused || forced }.collect { on ->
            progress.animateTo(if (on) 1f else 0f, tween(millis, easing = Tokens.ease))
        }
    }
    val colors = Marquee.colors
    return this
        .onFocusChanged { focused = it.isFocused }
        .layout { measurable, constraints ->
            val placeable = measurable.measure(constraints)
            layout(placeable.width, placeable.height) {
                // The focused node paints above its neighbours so its halo is not cut by them.
                placeable.place(0, 0, zIndex = progress.value)
            }
        }.graphicsLayer {
            val s = 1f + (scale - 1f) * progress.value
            scaleX = s
            scaleY = s
        }.drawBehind {
            val p = progress.value
            if (p > 0f) {
                val corner = radius.toPx()
                val ring = Tokens.focusRing.toPx()
                val spread = ring * 2 + Tokens.focusHalo.toPx()
                drawRoundRect(
                    color = colors.halo.copy(alpha = colors.halo.alpha * p),
                    topLeft = Offset(-spread, -spread),
                    size = Size(size.width + 2 * spread, size.height + 2 * spread),
                    cornerRadius = CornerRadius(corner + spread),
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
