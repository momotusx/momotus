package app.marquee.ui.tv

import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.tween
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.gestures.BringIntoViewSpec
import androidx.compose.foundation.gestures.LocalBringIntoViewSpec
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import app.marquee.design.Marquee
import app.marquee.design.Tokens

/**
 * Where a focused child comes to rest when a list scrolls to show it. The default reveals the
 * child with the least movement, so the next tile never peeks past the focused one and the focused
 * row can sit at the very bottom of the screen. This puts the child's leading edge at [lead]
 * whenever it would be hidden, or the next item would not show [trail] of itself, and leaves the
 * list alone otherwise.
 */
@OptIn(ExperimentalFoundationApi::class)
class AnchoredSpec(
    private val lead: Float,
    private val trail: Float,
    override val scrollAnimationSpec: AnimationSpec<Float>,
) : BringIntoViewSpec {
    override fun calculateScrollDistance(
        offset: Float,
        size: Float,
        containerSize: Float,
    ): Float = scrollDistance(offset, size, containerSize, lead, trail)
}

/** The scroll distance for a child at [offset] of [size] in a container: see [AnchoredSpec]. */
internal fun scrollDistance(
    offset: Float,
    size: Float,
    containerSize: Float,
    lead: Float,
    trail: Float,
): Float = if (offset < lead || offset + size + trail > containerSize) offset - lead else 0f

/** Applies [AnchoredSpec] to the scrollable lists inside [content]. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AnchoredScrolling(
    lead: Dp,
    trail: Dp,
    content: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val millis = Marquee.focusMs
    val spec =
        remember(density, lead, trail, millis) {
            AnchoredSpec(
                lead = with(density) { lead.toPx() },
                trail = with(density) { trail.toPx() },
                scrollAnimationSpec = tween(millis, easing = Tokens.ease),
            )
        }
    CompositionLocalProvider(LocalBringIntoViewSpec provides spec, content = content)
}
