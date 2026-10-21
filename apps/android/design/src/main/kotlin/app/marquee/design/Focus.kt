package app.marquee.design

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.layout
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
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
    // The keyboard ring is the key blue (design/DECISION.md); the TV form has no key color, so its ring stays white.
    val ringColor = if (Marquee.form == Form.Tv) colors.accent else colors.key
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
                    color = ringColor.copy(alpha = p),
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

/**
 * Touch gets a pressed state, TV none (its focus ring says where the press goes). A grey veil at
 * a fifth: it darkens the white primary and lightens a dark surface, so it reads on both.
 */
@Composable
private fun pressIndication(): IndicationNodeFactory? = if (Marquee.form == Form.Tv) null else PressVeil

private object PressVeil : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = PressVeilNode(interactionSource)

    override fun equals(other: Any?): Boolean = other === this

    override fun hashCode(): Int = 1
}

private class PressVeilNode(
    private val source: InteractionSource,
) : Modifier.Node(),
    DrawModifierNode {
    private var pressed = false

    override fun onAttach() {
        coroutineScope.launch {
            var held = 0
            source.interactions.collect {
                when (it) {
                    is PressInteraction.Press -> held++
                    is PressInteraction.Release, is PressInteraction.Cancel -> held--
                }
                if ((held > 0) != pressed) {
                    pressed = held > 0
                    invalidateDraw()
                }
            }
        }
    }

    override fun ContentDrawScope.draw() {
        drawContent()
        if (pressed) drawRect(PRESS_VEIL)
    }
}

private val PRESS_VEIL = Color(0x33808080)

/** Focusable and clickable (D-pad center and Enter click on TV), with no ripple and a pressed state on touch. */
@Composable
fun Modifier.focusClickable(
    onClick: () -> Unit,
    // Null for a node that says what it is itself (a field's button reads as an edit box).
    role: Role? = Role.Button,
    enabled: Boolean = true,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
): Modifier =
    if (onLongClick == null) {
        clickable(
            interactionSource = remember { MutableInteractionSource() },
            indication = pressIndication(),
            enabled = enabled,
            role = role,
            onClick = onClick,
        )
    } else {
        // Holding select on a remote, or pressing and holding on a touch screen.
        combinedClickable(
            interactionSource = remember { MutableInteractionSource() },
            indication = pressIndication(),
            enabled = enabled,
            role = role,
            onLongClickLabel = longClickLabel,
            onLongClick = onLongClick,
            onClick = onClick,
        )
    }

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
        indication = pressIndication(),
        role = role,
        onClick = onClick,
    )

/**
 * For an on/off control: a screen reader hears a switch it can always press. A selected choice of
 * [focusSelectable] loses its click action there, so it cannot turn a setting off.
 */
@Composable
fun Modifier.focusToggleable(
    on: Boolean,
    onToggle: () -> Unit,
): Modifier =
    toggleable(
        value = on,
        interactionSource = remember { MutableInteractionSource() },
        indication = pressIndication(),
        role = Role.Switch,
        onValueChange = { onToggle() },
    )

/**
 * For a text field: Up and Down leave it, as on every other control, instead of moving the cursor
 * to the start or the end. Without it a remote cannot get out of a focused field (seen on the TV
 * emulator); Left and Right keep moving the cursor.
 */
@Composable
fun Modifier.dpadExits(): Modifier {
    val focus = LocalFocusManager.current
    return onPreviewKeyEvent { event ->
        if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
        when (event.key) {
            Key.DirectionUp -> focus.moveFocus(FocusDirection.Up)
            Key.DirectionDown -> focus.moveFocus(FocusDirection.Down)
            else -> false
        }
    }
}

/**
 * Up and Down scroll [state] by about a quarter of a TV screen while it can move that way; at an
 * edge they pass on, so focus moves out as usual. For a focusable block of text that may run long.
 */
fun scrollByKey(
    event: KeyEvent,
    state: ScrollState,
    scope: CoroutineScope,
): Boolean {
    if (event.type != KeyEventType.KeyDown) return false
    val down =
        when (event.key) {
            Key.DirectionDown -> true
            Key.DirectionUp -> false
            else -> return false
        }
    val can = if (down) state.canScrollForward else state.canScrollBackward
    if (can) scope.launch { state.animateScrollBy(if (down) SCROLL_STEP else -SCROLL_STEP) }
    return can
}

private const val SCROLL_STEP = 240f

/**
 * D-pad presses in the app's window, counted by the TV shell. A focus asked for before a press never
 * lands after it: on a slow frame the viewer's first move comes first, and a late first focus would
 * take it back (the web's autoFocusSoon, d8aa39f1).
 */
object KeyPresses {
    var count = 0
        private set

    fun pressed() {
        count++
    }
}

/**
 * A deferred first focus: asks [requester] for focus once per frame (a node composed now is attached
 * a frame or two later) for up to [frames] frames, until it takes it. Ends when a D-pad key went down
 * since it was asked: the viewer's move stands.
 */
suspend fun focusSoon(
    requester: FocusRequester,
    frames: Int = 1,
) {
    val asked = KeyPresses.count
    repeat(frames) {
        withFrameNanos { }
        if (KeyPresses.count != asked) return
        if (runCatching { requester.requestFocus() }.getOrDefault(false)) return
    }
}
