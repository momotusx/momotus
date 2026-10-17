package app.marquee.design

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

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
    focusRequester: FocusRequester?,
    onFocusChange: ((Boolean) -> Unit)?,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val height = if (tv) (if (big) 84.dp else 72.dp) else (if (big) 56.dp else 48.dp)
    val pad = if (tv) (if (big) 36.dp else 30.dp) else (if (big) 24.dp else 20.dp)
    val gap = if (tv) 14.dp else 8.dp
    val corner = height / 2
    val shape = RoundedCornerShape(corner)
    val background = if (kind == ButtonKind.Primary) colors.accent else colors.text.copy(alpha = 0.12f)
    val content = if (kind == ButtonKind.Primary) colors.onAccent else colors.text
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
                        Modifier.border(2.dp, colors.text.copy(alpha = 0.22f), shape)
                    } else {
                        Modifier
                    },
                ).focusClickable(onClick, Role.Button, enabled)
                .padding(horizontal = pad),
        horizontalArrangement = Arrangement.spacedBy(gap, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) MqIcon(icon, tint = content)
        MqText(label, color = content, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** The one filled, accent-colored action of a screen. */
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
) = ButtonShell(label, onClick, modifier, icon, ButtonKind.Primary, big, enabled, focusRequester, onFocusChange)

@Composable
fun SecondaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    big: Boolean = false,
    enabled: Boolean = true,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
) = ButtonShell(label, onClick, modifier, icon, ButtonKind.Secondary, big, enabled, focusRequester, onFocusChange)

/** A pill for filters and choices. Selected is shown by fill and weight, not by the accent color. */
@Composable
fun Chip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
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
                .heightIn(min = target)
                .focusSelectable(selected, onClick)
                .padding(vertical = (target - pill) / 2),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .heightIn(min = pill)
                .clip(shape)
                // Opaque when not selected, so the focus halo of a neighbour does not show through.
                .background(if (selected) colors.raised else colors.bg)
                .border(2.dp, if (selected) colors.text.copy(alpha = 0.5f) else colors.line, shape)
                .padding(horizontal = if (tv) 24.dp else 14.dp),
            contentAlignment = Alignment.Center,
        ) {
            MqText(
                label,
                role = TextRole.Callout,
                color = if (selected) colors.text else colors.muted,
                weight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
                maxLines = 1,
            )
        }
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
                .background(colors.text),
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

/** The explained empty or failed state: what happened, why, and a fix. */
@Composable
fun StateMessage(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    actionFocus: FocusRequester? = null,
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
        if (actionLabel != null && onAction != null) {
            SecondaryButton(actionLabel, onAction, focusRequester = actionFocus)
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
