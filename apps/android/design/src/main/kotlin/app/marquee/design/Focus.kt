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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.shadow.Shadow
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
import androidx.compose.ui.platform.LocalGraphicsContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpOffset
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.sin

/**
 * The focus look from design/DECISION.md (v3). On TV, Apple's: the item scales up; a card, a button
 * or a chip (a scale of [Tokens.FOCUS_SCALE_TILE] or more, [rise]) rises [Tokens.focusRise] over the
 * lift shadow, and artwork ([art]: tiles, stills, avatars) rises [Tokens.focusRiseArt] over the deeper art
 * shadow and catches the sheen. No ring and no colored edge: buttons and rows turn white with dark
 * words themselves ([focusPalette]); a target with no fill of its own takes a faint white platter and
 * neither rises nor casts a shadow, so focus never shows as nothing and no shadow shows through.
 * The player is no exception (design/DECISION.md, TV focus: never a ring). On the phone, focus comes only from a keyboard or a D-pad, and shows as the ring alone:
 * one 4 dp stroke in the key blue right outside the edge, with no gap, no scale and no fill change.
 *
 * Cost on weak GPUs: the shadow is one blurred shape, the rest a few rounded rects, only while
 * focused. The focus flag and the animation are read inside the graphics layer and the draw
 * lambdas, so a focus change redraws this node and never recomposes the caller. The scale and the
 * rise are drawn, not laid out or set on a layer, so focus search and the scrolls that bring a card into
 * view keep the card's place. Content that scales,
 * rises or draws outside its bounds needs room around it (`Tokens` tile `pad`, [liftRoom]), because
 * parents clip. A new focus target cancels the fade of the old one, so a held D-pad leaves no trail.
 *
 * Place this before `clickable` or `focusable` in the chain: it observes the focus target after it.
 *
 * @param forced shows the focused look without focus (design gallery, screenshots).
 * @param lift when a component wraps the focused node (a tile with a caption), it lifts its own
 * root above its siblings with [lifted] and passes the flag here; without it this node lifts itself.
 * @param art artwork: the art rise and shadow, and the sheen clipped to [radius].
 * @param rise false for a target that only scales and inverts, such as a tab inside its bar.
 */
@Composable
fun Modifier.marqueeFocus(
    radius: Dp,
    scale: Float = Tokens.FOCUS_SCALE_CARD,
    forced: Boolean = false,
    lift: MutableState<Boolean>? = null,
    art: Boolean = false,
    rise: Boolean = true,
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
    val tv = Marquee.form == Form.Tv
    val ringWidth = ringWidth(tv)
    val halo = Marquee.focusHalo
    val rises = tv && rise && scale >= Tokens.FOCUS_SCALE_TILE
    val shape = RoundedCornerShape(radius)
    val graphics = LocalGraphicsContext.current
    // Apple's lift (the web's --lift-focus-edge, 0 30px 60px at .6), and for art the deeper one
    // (--lift-focus-art, 0 44px 88px at .55).
    val shadow =
        Shadow(
            radius = if (art) Tokens.focusArtShadowBlur else halo * 2,
            color = Color.Black,
            offset = DpOffset(0.dp, if (art) Tokens.focusArtShadowY else halo),
            alpha = if (art) Tokens.FOCUS_ART_SHADOW_ALPHA else colors.halo.alpha,
        )
    return this
        .onFocusChanged { focused = it.isFocused }
        .then(if (lift == null) Modifier.lifted(raised) else Modifier)
        .then(
            // The phone's keyboard focus is the ring alone: nothing scales there.
            if (tv) {
                // Drawn, not a layer's transform: the scroll that brings a focused node into view reads its bounds through
                // every layer, so a scaled layer grew the tile's top on each frame of the tween, and a page that pivots on
                // it (a TV's default, as on Discover) went up and down with every move along a row (FocusStillTest).
                // The plain layer keeps the redraw of each frame inside this node.
                Modifier.graphicsLayer().drawWithContent {
                    scale(1f + (scale - 1f) * progress.value) { this@drawWithContent.drawContent() }
                }
            } else {
                Modifier
            },
        ).then(
            if (rises) {
                Modifier
                    .drawWithContent {
                        val up = (if (art) Tokens.focusRiseArt else Tokens.focusRise).toPx() * progress.value
                        translate(top = -up) { this@drawWithContent.drawContent() }
                    }.drawBehind {
                        // The shadow fades through the painter's alpha, never its own: Compose keeps one blurred
                        // bitmap per Shadow, alpha included, until the app leaves the screen, so an animated
                        // alpha kept one a frame and filled the Fire TV's heap (FocusShadowTest). The painter
                        // comes from the window's shadow context as it is now (a trim replaces it).
                        val p = progress.value
                        if (p <= 0f) return@drawBehind
                        with(graphics.shadowContext.createDropShadowPainter(shape, shadow)) { draw(size, alpha = p) }
                    }
            } else {
                Modifier
            },
        ).drawBehind {
            val p = progress.value
            if (p <= 0f) return@drawBehind
            val corner = radius.toPx()
            if (tv) {
                drawRoundRect(
                    color = PLATTER.copy(alpha = PLATTER.alpha * p),
                    size = size,
                    cornerRadius = CornerRadius(corner),
                )
            }
            val ring = ringWidth.toPx()
            if (ring <= 0f) return@drawBehind
            // The phone's key ring, right outside the edge.
            val at = ring / 2
            drawRoundRect(
                color = colors.key.copy(alpha = p),
                topLeft = Offset(-at, -at),
                size = Size(size.width + 2 * at, size.height + 2 * at),
                cornerRadius = CornerRadius(corner + at),
                style = Stroke(width = ring),
            )
        }.then(
            if (art && tv) {
                Modifier.drawWithCache {
                    val clip =
                        Path().apply {
                            addRoundRect(
                                RoundRect(Rect(Offset.Zero, size), CornerRadius(radius.toPx())),
                            )
                        }
                    // CSS's linear-gradient(125deg, ...): its line runs through the center at 125
                    // degrees from up, as long as the box's projection on it.
                    val a = Math.toRadians(SHEEN_DEG)
                    val dx = sin(a).toFloat()
                    val dy = -cos(a).toFloat()
                    val half = (abs(size.width * dx) + abs(size.height * dy)) / 2
                    val c = Offset(size.width / 2, size.height / 2)
                    val sheen =
                        Brush.linearGradient(
                            0f to Color.White.copy(alpha = 0.26f),
                            0.32f to Color.White.copy(alpha = 0.06f),
                            0.52f to Color.Transparent,
                            start = Offset(c.x - dx * half, c.y - dy * half),
                            end = Offset(c.x + dx * half, c.y + dy * half),
                        )
                    onDrawWithContent {
                        drawContent()
                        val p = progress.value
                        if (p > 0f) clipPath(clip) { drawRect(sheen, alpha = p) }
                    }
                }
            } else {
                Modifier
            },
        )
}

/**
 * The width of the ring a focus target strokes: none on TV, the player included, which shows focus by the lift and
 * the white invert; the 4 dp ring on the phone (the key blue).
 */
internal fun ringWidth(tv: Boolean): Dp = if (tv) 0.dp else Tokens.focusRing

/**
 * How many buttons in this tree hold focus (MarqueeTheme provides it). On TV a white primary yields
 * its white while one does, so focus stays the one white thing on the screen.
 */
val LocalButtonFocus = staticCompositionLocalOf<MutableIntState?> { null }

/**
 * Counts this button in [LocalButtonFocus] while [focused]. ponytail: one count for the whole screen,
 * not per action row; a primary yields to a focused button anywhere, which no screen has apart from
 * its own row today. Scope it to the row (a provider per row) if one ever does.
 */
@Composable
fun ButtonFocused(focused: Boolean) {
    val count = LocalButtonFocus.current ?: return
    DisposableEffect(focused, count) {
        if (focused) count.intValue++
        onDispose { if (focused) count.intValue-- }
    }
}

/** The faint platter behind a focused target that has no fill of its own (a link, a text block). */
private val PLATTER = Color(0x29FFFFFF)

/** The sheen's angle, as the frames and the web draw it. */
private const val SHEEN_DEG = 125.0

/**
 * The palette a focus target draws with (design/DECISION.md, TV focus): on TV a focused button or
 * row turns white with dark words, so its fill is `primary` and every word, glyph and edge inside
 * it is dark, the player included; elsewhere the palette as it is. Give the
 * result to the row's own fill and, through [FocusInk], to its content.
 */
@Composable
@ReadOnlyComposable
fun focusPalette(focused: Boolean): Palette {
    val colors = Marquee.colors
    return if (focused && Marquee.form == Form.Tv) colors.inverted() else colors
}

/**
 * A row listed though none of them plays (the Versions empty state): its words drop to `muted`, a level whose
 * contrast is tested, never to an alpha under it, since the row can still be started.
 */
fun Palette.dimmed(): Palette = copy(text = muted)

/** The TV invert: `primary` for the fills, `onPrimary` for the words, at Apple's dark levels. */
fun Palette.inverted(): Palette =
    copy(
        surface = primary,
        raised = primary,
        glass = primary,
        glassStrong = primary,
        edge = Color.Transparent,
        control = Color.Transparent,
        line = onPrimary.copy(alpha = 0.1f),
        lineStrong = onPrimary.copy(alpha = 0.2f),
        text = onPrimary,
        muted = onPrimary.copy(alpha = 0.64f),
        dim = onPrimary.copy(alpha = 0.64f),
        // White words and status glyphs (accent, star, ok, warn, bad are white in v3) go dark.
        accent = onPrimary,
        star = onPrimary,
        ok = onPrimary,
        warn = onPrimary.copy(alpha = 0.64f),
        bad = onPrimary,
    )

/** Draws [content] with [colors] (a [focusPalette]), so its default text and glyph colors follow. */
@Composable
fun FocusInk(
    colors: Palette,
    content: @Composable () -> Unit,
) {
    CompositionLocalProvider(LocalPalette provides colors, content = content)
}

/**
 * Room for a focused child to scale, rise and cast its shadow in a row that clips (a lazy row of
 * chips): the row is laid out [room] taller at both ends and placed back, so the page around it
 * does not move (Apple's padding and negative-margin form). Pair it with a vertical content padding
 * of the same [room].
 */
fun Modifier.liftRoom(room: Dp): Modifier =
    layout { measurable, constraints ->
        val r = room.roundToPx()
        val grown =
            constraints.copy(
                minHeight = constraints.minHeight + 2 * r,
                maxHeight = if (constraints.hasBoundedHeight) constraints.maxHeight + 2 * r else constraints.maxHeight,
            )
        val placeable = measurable.measure(grown)
        layout(placeable.width, (placeable.height - 2 * r).coerceAtLeast(0)) { placeable.place(0, -r) }
    }

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
 * Touch gets a pressed state, TV none (its focus says where the press goes). A grey veil at
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
