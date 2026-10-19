package app.marquee.design

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.core.net.toUri
import kotlinx.coroutines.flow.first

private enum class ButtonKind { Primary, Secondary }

@Composable
private fun ButtonShell(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier,
    icon: ImageVector?,
    kind: ButtonKind,
    big: Boolean,
    enabled: Boolean,
    selected: Boolean,
    focusRequester: FocusRequester?,
    onFocusChange: ((Boolean) -> Unit)?,
    busy: Boolean = false,
    spoken: String? = null,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val height = if (tv) (if (big) 84.dp else 72.dp) else (if (big) 56.dp else 48.dp)
    val pad = if (tv) (if (big) 36.dp else 30.dp) else (if (big) 24.dp else 20.dp)
    val gap = if (tv) 14.dp else 8.dp
    val corner = height / 2
    val shape = RoundedCornerShape(corner)
    // The one primary action is the white pill (design/DECISION.md v2). A secondary is raised with
    // the control edge; a selected toggle (Watched, In Library) is lighter. The ring is for focus alone.
    val background =
        when {
            kind == ButtonKind.Primary -> colors.primary
            selected -> colors.text.copy(alpha = 0.22f).compositeOver(colors.raised)
            else -> colors.raised
        }
    val content = if (kind == ButtonKind.Primary) colors.onPrimary else colors.text
    Row(
        modifier =
            modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .alpha(if (enabled) 1f else 0.55f)
                .marqueeFocus(corner)
                .then(
                    if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier,
                ).heightIn(min = height) // grows with a large system font instead of cutting the label
                .clip(shape)
                .background(background)
                .then(
                    if (kind == ButtonKind.Secondary) {
                        Modifier.border(2.dp, colors.control, shape)
                    } else {
                        Modifier
                    },
                ).then(if (spoken != null) Modifier.semantics { contentDescription = spoken } else Modifier)
                .focusClickable(onClick, Role.Button, enabled)
                .padding(horizontal = pad, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(gap, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) {
            Spinner(size = if (tv) 26.dp else 18.dp)
        } else if (icon != null) {
            MqIcon(icon, tint = content)
        }
        // A label that does not fit (a small phone, a large font) takes a second line, never an ellipsis.
        MqText(
            label,
            // With [spoken] the button says that alone, not the label after it.
            modifier = if (spoken != null) Modifier.clearAndSetSemantics { } else Modifier,
            color = content,
            weight = FontWeight.SemiBold,
            maxLines = 2,
            textAlign = TextAlign.Center,
        )
    }
}

/** The one primary action of a screen: the white pill. */
@Composable
fun PrimaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    big: Boolean = false,
    enabled: Boolean = true,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
) = ButtonShell(label, onClick, modifier, icon, ButtonKind.Primary, big, enabled, false, focusRequester, onFocusChange)

/**
 * @param selected a toggle that is on (Watched, In Library): drawn with the raised fill.
 * @param busy its request runs: a spinner in place of the icon; the button stays, so focus does too.
 * @param spoken what a screen reader says in place of the label (the label with what it acts on).
 */
@Composable
fun SecondaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    big: Boolean = false,
    enabled: Boolean = true,
    selected: Boolean = false,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
    busy: Boolean = false,
    spoken: String? = null,
) = ButtonShell(
    label,
    onClick,
    modifier,
    icon,
    ButtonKind.Secondary,
    big,
    enabled,
    selected,
    focusRequester,
    onFocusChange,
    busy,
    spoken,
)

/**
 * A pill for filters and choices. Selected is the white pill with a check, never color alone.
 * [switch] makes it an on/off control on its own rather than one choice of a group.
 */
@Composable
fun Chip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
    switch: Boolean = false,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    // The pill is 36 dp on a phone inside a 48 dp touch target; on TV they are the same.
    val pill = if (tv) 52.dp else 36.dp
    val target = if (tv) pill else 48.dp
    val shape = RoundedCornerShape(pill / 2)
    Box(
        modifier =
            modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(target / 2, Tokens.FOCUS_SCALE_CARD)
                .then(
                    if (onFocusChange != null) Modifier.onFocusChanged { onFocusChange(it.isFocused) } else Modifier,
                ).heightIn(min = target)
                .then(
                    if (switch) {
                        Modifier.focusToggleable(
                            selected,
                            onClick,
                        )
                    } else {
                        Modifier.focusSelectable(selected, onClick)
                    },
                ).padding(vertical = (target - pill) / 2),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .heightIn(min = pill)
                .clip(shape)
                // Opaque when not selected, so the focus halo of a neighbour does not show through.
                .background(if (selected) colors.primary else colors.raised)
                // The edge is the control token (3:1 against the page), so an unselected pill does not
                // vanish; the ring is for focus alone.
                .then(if (selected) Modifier else Modifier.border(2.dp, colors.control, shape))
                .padding(horizontal = if (tv) 24.dp else 14.dp),
            contentAlignment = Alignment.Center,
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(if (tv) 10.dp else 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (selected) MqIcon(Icons.Check, tint = colors.onPrimary, size = if (tv) 24.dp else 16.dp)
                MqText(
                    label,
                    role = TextRole.Callout,
                    color = if (selected) colors.onPrimary else colors.muted,
                    weight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
                    maxLines = 1,
                )
            }
        }
    }
}

/**
 * Scrolls [state] so item [index] is wholly on screen, at first layout and whenever [index] changes:
 * a selected chip far along a row (season 38, opened from Calendar) is never off screen.
 */
@Composable
fun KeepInView(
    state: LazyListState,
    index: Int,
) {
    LaunchedEffect(state, index) {
        if (index < 0) return@LaunchedEffect
        val info = snapshotFlow { state.layoutInfo }.first { it.totalItemsCount > index }
        val item = info.visibleItemsInfo.firstOrNull { it.index == index }
        val shown =
            item != null && item.offset >= 0 &&
                item.offset + item.size <= info.viewportEndOffset - info.afterContentPadding
        if (!shown) state.scrollToItem(index)
    }
}

/** A watched-progress bar for the foot of a tile. */
@Composable
fun ProgressBar(
    progress: Float,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val height = if (Marquee.form == Form.Tv) 6.dp else 4.dp
    Box(modifier.fillMaxWidth().height(height).background(colors.scrim)) {
        Box(
            Modifier
                .fillMaxWidth(progress.coerceIn(0f, 1f))
                .height(height)
                .background(colors.accent),
        )
    }
}

/**
 * One setup check: a status mark plus its name. The mark differs in shape as well as color (a
 * check, a triangle, a cross, an empty circle) and screen readers get the status as words.
 */
@Composable
fun StatusLine(
    kind: Status,
    label: String,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val (icon, tint, words) =
        when (kind) {
            Status.Ok -> Triple(Icons.CircleCheck, colors.ok, "OK")
            Status.Warn -> Triple(Icons.TriangleAlert, colors.warn, "Warning")
            Status.Bad -> Triple(Icons.CircleX, colors.bad, "Problem")
            Status.Idle -> Triple(Icons.Circle, colors.muted, "Not checked")
        }
    Row(
        modifier.semantics(mergeDescendants = true) { stateDescription = words },
        horizontalArrangement = Arrangement.spacedBy(if (Marquee.form == Form.Tv) 12.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(icon, tint = tint)
        MqText(label, color = colors.text)
    }
}

enum class Status { Ok, Warn, Bad, Idle }

/**
 * The explained empty or failed state: what happened, why, and a fix. A second way out
 * ([secondaryLabel]) sits beside the fix as a secondary button.
 */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun StateMessage(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    actionFocus: FocusRequester? = null,
    secondaryLabel: String? = null,
    onSecondary: (() -> Unit)? = null,
    actionIcon: ImageVector? = null,
    secondaryIcon: ImageVector? = null,
    /** Where the D-pad goes from the second button (a TV list beside the message). */
    secondaryModifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    Column(
        modifier = modifier.padding(vertical = if (tv) 18.dp else 10.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(if (tv) 16.dp else 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (icon != null) MqIcon(icon, tint = Marquee.colors.bad)
            MqText(title, role = TextRole.Title3)
        }
        MqText(body, role = TextRole.Callout, color = Marquee.colors.muted)
        // The fix is the state's one primary: the white pill, as on the web. Coming into the
        // buttons from anywhere (Down from the TV pill) lands on it, not on its nearest neighbour.
        val primary = actionFocus ?: remember { FocusRequester() }
        val hasPrimary = actionLabel != null && onAction != null
        FlowRow(
            modifier = Modifier.focusProperties { if (hasPrimary) enter = { primary } }.focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
            verticalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
        ) {
            if (actionLabel != null && onAction != null) {
                PrimaryButton(actionLabel, onAction, icon = actionIcon, focusRequester = primary)
            }
            if (secondaryLabel != null && onSecondary != null) {
                SecondaryButton(secondaryLabel, onSecondary, secondaryModifier, icon = secondaryIcon)
            }
        }
    }
}

/** A placeholder block that pulses while content loads (static when animations are off). */
@Composable
fun Skeleton(modifier: Modifier = Modifier) {
    val reduced = LocalReducedMotion.current
    val pulse =
        rememberInfiniteTransition(label = "skeleton").animateFloat(
            initialValue = 0.55f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(1600), RepeatMode.Reverse),
            label = "pulse",
        )
    Box(
        modifier
            // Read in the draw phase: the pulse redraws this block and recomposes nothing.
            .graphicsLayer { alpha = if (reduced) 1f else pulse.value }
            .clip(RoundedCornerShape(Marquee.radius.m))
            .background(Marquee.colors.surface),
    )
}

/** Gives a node a spoken label. */
fun Modifier.describe(text: String): Modifier = semantics { contentDescription = text }

/** A turning arc: something is being waited for (a source, the buffer). Still when animations are off. */
@Composable
fun Spinner(
    modifier: Modifier = Modifier,
    size: Dp = if (LocalForm.current == Form.Tv) 40.dp else 28.dp,
) {
    val color = Marquee.colors.text
    val turn =
        rememberInfiniteTransition(label = "spinner").animateFloat(
            initialValue = 0f,
            targetValue = 360f,
            animationSpec = infiniteRepeatable(tween(1200, easing = LinearEasing), RepeatMode.Restart),
            label = "turn",
        )
    val reduced = LocalReducedMotion.current
    Canvas(modifier.size(size)) {
        val stroke = size.toPx() / 9f
        rotate(if (reduced) 0f else turn.value) {
            drawArc(
                color = color,
                startAngle = 0f,
                sweepAngle = 270f,
                useCenter = false,
                topLeft = Offset(stroke / 2, stroke / 2),
                size = Size(this.size.width - stroke, this.size.height - stroke),
                style = Stroke(width = stroke, cap = StrokeCap.Round),
            )
        }
    }
}

private val Status.tint: Color
    @Composable get() =
        when (this) {
            Status.Ok -> Marquee.colors.ok
            Status.Warn -> Marquee.colors.warn
            Status.Bad -> Marquee.colors.bad
            Status.Idle -> Marquee.colors.muted
        }

@Composable
private fun StatusDot(tone: Status) {
    val size = if (Marquee.form == Form.Tv) 12.dp else 8.dp
    Box(Modifier.size(size).clip(CircleShape).background(tone.tint))
}

/**
 * A version's line (plans/m13-stream-choice.md 3.7): the words after an icon whose shape says the tone
 * (a check, a triangle, an octagon), so the tone never rests on color alone.
 */
@Composable
fun ToneWord(
    text: String,
    tone: Status,
    modifier: Modifier = Modifier,
    maxLines: Int = 2,
) {
    val tv = Marquee.form == Form.Tv
    // The icon stays by the first line when the words take two.
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(if (tv) 10.dp else 6.dp),
        verticalAlignment = Alignment.Top,
    ) {
        MqIcon(
            when (tone) {
                Status.Bad -> Icons.OctagonAlert
                Status.Warn -> Icons.TriangleAlert
                else -> Icons.CircleCheck
            },
            tint = tone.tint,
            size = if (tv) 26.dp else 18.dp,
        )
        MqText(text, role = TextRole.Callout, maxLines = maxLines)
    }
}

/** A status as a word with a dot in front (the word always carries the meaning; the dot only tints it). */
@Composable
fun StatusWord(
    text: String,
    tone: Status,
    modifier: Modifier = Modifier,
    color: Color = Marquee.colors.text,
    /** A state said in a whole sentence takes the lines it needs. */
    maxLines: Int = 2,
) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(if (Marquee.form == Form.Tv) 10.dp else 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StatusDot(tone)
        // A second line on a small phone or a large font, never an ellipsis over what happened.
        MqText(text, role = TextRole.Callout, color = color, maxLines = maxLines)
    }
}

/**
 * The version a title will play, under its Play button, as the web writes it: the facts, then a status
 * with a dot, then the source ("1080p  1.6 GB  ● Torrent, 10 seeds  via Torrentio"). It wraps at
 * the gaps and never inside a fact. Screen readers get [spoken] as one sentence.
 */
@Composable
fun ChosenVersion(
    facts: List<String>,
    status: String,
    tone: Status,
    via: String,
    spoken: String,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    FlowRow(
        modifier.clearAndSetSemantics { contentDescription = spoken },
        horizontalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        for (fact in facts) MqText(fact, role = TextRole.Callout, maxLines = 1)
        StatusWord(status, tone)
        MqText("via $via", role = TextRole.Callout, color = Marquee.colors.muted, maxLines = 1)
    }
}

/**
 * A status line: a spinner and [text] while [busy] (16 dp, 24 dp on TV), else a dot in [tone]. The
 * text wraps to two lines unless [maxLines] says more.
 */
@Composable
fun StatusNote(
    text: String,
    modifier: Modifier = Modifier,
    tone: Status = Status.Idle,
    busy: Boolean = false,
    /** Why nothing plays is said whole ("Marquee found 9 versions. This version of the app..."). */
    maxLines: Int = 2,
) {
    val tv = Marquee.form == Form.Tv
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) Spinner(size = if (tv) 24.dp else 16.dp) else StatusDot(tone)
        MqText(text, role = TextRole.Callout, color = Marquee.colors.muted, maxLines = maxLines)
    }
}

/**
 * A round control. With [showLabel] its label shows under the ring (Details, Trailer, Library);
 * without it the label is only spoken (the player's transport). [main] is the white one (play and
 * pause); [selected] is a held toggle, drawn white as well and reported to screen readers.
 */
@Composable
fun RoundButton(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    main: Boolean = false,
    selected: Boolean = false,
    showLabel: Boolean = false,
    enabled: Boolean = true,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    longClickLabel: String? = null,
    /** A short text in the middle of the icon: the seconds a skip button jumps ("10"). */
    badge: String? = null,
    /** What a screen reader says, when the visible label needs its target ("Remove Torrentio"). */
    spoken: String = label,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val size =
        when {
            main -> if (tv) 104.dp else 60.dp
            showLabel -> if (tv) 72.dp else 48.dp
            else -> if (tv) 84.dp else 48.dp
        }
    val glyph =
        when {
            main -> if (tv) 44.dp else 26.dp
            showLabel -> if (tv) 30.dp else 20.dp
            else -> if (tv) 36.dp else 22.dp
        }
    val lit = main || selected
    var focused by remember { mutableStateOf(false) }
    Column(
        modifier.alpha(if (enabled) 1f else 0.55f),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(if (tv) 10.dp else 6.dp),
    ) {
        Box(
            Modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .size(size)
                .marqueeFocus(size / 2)
                .onFocusChanged {
                    focused = it.isFocused
                    onFocusChange?.invoke(it.isFocused)
                }.clip(CircleShape)
                .background(if (lit) colors.primary else colors.raised)
                .then(if (lit) Modifier else Modifier.border(2.dp, colors.control, CircleShape))
                .semantics {
                    contentDescription = spoken
                    if (selected) stateDescription = "On"
                }.focusClickable(onClick, Role.Button, enabled, onLongClick, longClickLabel),
            contentAlignment = Alignment.Center,
        ) {
            MqIcon(icon, tint = if (lit) colors.onPrimary else colors.text, size = glyph)
            if (badge != null) {
                // The label already says the seconds; the badge is only for the eye. A third of the
                // glyph, so it sits inside the arrow as on the web rather than over it.
                BasicText(
                    badge,
                    modifier = Modifier.clearAndSetSemantics { },
                    style =
                        TextStyle(
                            color = if (lit) colors.onPrimary else colors.text,
                            fontSize = with(LocalDensity.current) { (glyph * 0.34f).toSp() },
                            fontFamily = MarqueeFonts.text,
                            fontWeight = FontWeight.Bold,
                        ),
                )
            }
        }
        if (showLabel) {
            MqText(
                label,
                // Spoken once, as the ring's name; on touch the word presses the button too.
                modifier =
                    Modifier
                        .clearAndSetSemantics { }
                        .then(
                            if (!tv && enabled) {
                                Modifier.pointerInput(onClick) { detectTapGestures { onClick() } }
                            } else {
                                Modifier
                            },
                        ),
                role = TextRole.Caption,
                color = if (focused) colors.text else colors.muted,
                weight = FontWeight.SemiBold,
                // A narrow slot (equal widths in a row, a large font) wraps the word under its ring.
                maxLines = 2,
                textAlign = TextAlign.Center,
            )
        }
    }
}

/**
 * A row's header: a short white bar, the title, the source in `dim` and a hairline to the end
 * (design/DECISION.md v2, Section headers).
 */
@Composable
fun RowHeader(
    title: String,
    source: String?,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Row(
        modifier.fillMaxWidth().semantics(mergeDescendants = true) { heading() },
        horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(width = if (tv) 5.dp else 3.dp, height = if (tv) 34.dp else 20.dp)
                .clip(RoundedCornerShape(2.dp))
                .background(colors.text),
        )
        MqText(title, role = TextRole.Title3, maxLines = 1)
        if (source != null) MqText(source, role = TextRole.Callout, color = colors.dim, maxLines = 1)
        Box(Modifier.weight(1f).height(1.dp).background(colors.line))
    }
}

/**
 * A short label in accent spaced capitals (design/DECISION.md v2). Accent text keeps 4.66:1 only
 * on the page color, so over artwork the label sits in a chip filled with it.
 */
@Composable
fun Eyebrow(
    text: String,
    modifier: Modifier = Modifier,
    overArt: Boolean = false,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    MqText(
        text.uppercase(),
        modifier =
            modifier.then(
                if (overArt) {
                    Modifier
                        .clip(RoundedCornerShape(Marquee.radius.pill))
                        .background(colors.bg)
                        .padding(horizontal = if (tv) 16.dp else 10.dp, vertical = if (tv) 8.dp else 5.dp)
                } else {
                    Modifier
                },
            ),
        role = TextRole.Caption,
        color = colors.accent,
        weight = FontWeight.Bold,
        // A phone takes a second line rather than lose the subject ("#1 IN POPULAR MOVI...").
        maxLines = if (tv) 1 else 2,
        letterSpacing = 0.14.em,
    )
}

/**
 * A star with the IMDb rating, then the facts ("2026 · 145 min · Action, Adventure"): the meta
 * line of a hero and a title page. Without a rating it is the facts alone.
 */
@Composable
fun RatingLine(
    rating: String?,
    facts: String,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (rating != null) {
            MqIcon(StarFilled, tint = colors.star, size = if (tv) 26.dp else 15.dp)
            MqText(
                rating,
                modifier = Modifier.semantics { contentDescription = "IMDb $rating" },
                role = TextRole.Callout,
                weight = FontWeight.Bold,
                maxLines = 1,
            )
            MqText("·", modifier = Modifier.clearAndSetSemantics { }, role = TextRole.Callout, color = colors.dim)
        }
        MqText(facts, role = TextRole.Callout, color = colors.muted, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

/**
 * Opens a web page (a trailer's watch page, a sign-in link) in the app that handles it or a
 * browser. False when the device has neither (some TVs), so the caller can say so.
 */
fun openLink(
    context: android.content.Context,
    url: String,
): Boolean =
    try {
        context.startActivity(
            android.content
                .Intent(android.content.Intent.ACTION_VIEW, url.toUri())
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        true
    } catch (e: android.content.ActivityNotFoundException) {
        false
    }

/**
 * A title's facts as a key-value table on glass (design/DECISION.md v2, Title page): a `dim`
 * label and its value per row. Over artwork the strong glass keeps both readable.
 */
@Composable
fun FactsTable(
    facts: List<Pair<String, String>>,
    modifier: Modifier = Modifier,
) {
    if (facts.isEmpty()) return
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    FactsGrid(
        facts,
        modifier
            .clip(shape)
            .background(colors.glassStrong)
            .border(1.dp, colors.edge, shape)
            .padding(if (Marquee.form == Form.Tv) 28.dp else 16.dp),
    )
}

/**
 * The rows of [FactsTable] without its glass, for a card that is glass already. A label in
 * [tones] puts a status dot before its value ("Status: ● Direct link").
 */
@Composable
fun FactsGrid(
    facts: List<Pair<String, String>>,
    modifier: Modifier = Modifier,
    tones: Map<String, Status> = emptyMap(),
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Column(modifier, verticalArrangement = Arrangement.spacedBy(if (tv) 14.dp else 10.dp)) {
        for ((label, value) in facts) {
            Row(
                Modifier.semantics(mergeDescendants = true) {},
                horizontalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 12.dp),
                // A label stays on the first line of a value that wraps.
                verticalAlignment = Alignment.Top,
            ) {
                MqText(
                    label,
                    modifier = Modifier.width(if (tv) 170.dp else 104.dp),
                    role = TextRole.Caption,
                    color = colors.dim,
                    weight = FontWeight.SemiBold,
                    maxLines = 1,
                )
                tones[label]?.let { StatusDot(it) }
                MqText(value, role = TextRole.Caption, maxLines = 2)
            }
        }
    }
}

/** A small label in `dim` spaced capitals: a card's kicker, a table's column, a group's name. */
@Composable
fun CapsLabel(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Marquee.colors.dim,
) {
    MqText(
        text.uppercase(),
        modifier = modifier,
        role = TextRole.Caption,
        color = color,
        weight = FontWeight.Bold,
        maxLines = 2,
        letterSpacing = 0.12.em,
    )
}

/** A bar on glass with an edge: a group's name and count, or the hidden summary. */
@Composable
fun GlassBar(
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.m)
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.glass)
            .border(1.dp, colors.edge, shape)
            .padding(horizontal = if (tv) 22.dp else 16.dp, vertical = if (tv) 12.dp else 10.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 14.dp else 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/**
 * Fades the last [length] of the content toward its end (right, or bottom when [vertical]) while
 * [on]: the sign that a row or a block scrolls on.
 */
fun Modifier.endFade(
    on: Boolean,
    vertical: Boolean = false,
    length: Dp = 32.dp,
): Modifier =
    if (!on) {
        this
    } else {
        graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }.drawWithContent {
            drawContent()
            val px = length.toPx()
            val fade = listOf(Color.Black, Color.Transparent)
            val brush =
                if (vertical) {
                    Brush.verticalGradient(fade, startY = size.height - px, endY = size.height)
                } else {
                    Brush.horizontalGradient(fade, startX = size.width - px, endX = size.width)
                }
            drawRect(brush, blendMode = BlendMode.DstIn)
        }
    }
