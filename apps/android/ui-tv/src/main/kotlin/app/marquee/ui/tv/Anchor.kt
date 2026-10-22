package app.marquee.ui.tv

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.gestures.BringIntoViewSpec
import androidx.compose.foundation.gestures.LocalBringIntoViewSpec
import androidx.compose.foundation.gestures.ScrollableState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import app.marquee.design.RoomSpec

/**
 * Where a focused row comes to rest when the page scrolls to show it. The default reveals the
 * child with the least movement, so the focused row can sit at the very bottom of the screen. This
 * puts the child's leading edge at [lead] whenever it would be hidden, or [trail] under it would be
 * (a card's lines under its art), leaves the list alone otherwise, and does nothing while [hold] (the
 * page's own scroll to its top has the move, TvHome).
 *
 * Compose asks again on every frame of the scroll, with the child where it is by then. The rule alone
 * answers 0 as soon as the child is merely in view, so the page stopped dead halfway, at full speed
 * (TV Home, 2026-10-22: a row came up a third of the screen in three frames and stopped at the foot).
 * So a scroll once started goes on to [lead] until [list] stops scrolling: it arrived, or the list's
 * end held it. That state lives in this instance, so it is made once per list and reads [lead], [trail]
 * and [hold] as they are now. The move itself is Compose's spring: a spec's own animation is no longer
 * read ("Animation spec customization is no longer supported"; a 2 s linear one moved the page exactly
 * as the focus tween did, TvHomeScrollTest).
 */
@OptIn(ExperimentalFoundationApi::class)
class AnchoredSpec(
    private val list: ScrollableState,
    private val lead: () -> Float,
    private val trail: () -> Float = { 0f },
    private val hold: () -> Boolean = { false },
) : BringIntoViewSpec {
    private var travelling = false

    override fun calculateScrollDistance(
        offset: Float,
        size: Float,
        containerSize: Float,
    ): Float =
        // Asked from layout callbacks too: the reads must not subscribe them to the scroll.
        Snapshot.withoutReadObservation {
            if (!list.isScrollInProgress) travelling = false
            val lead = lead()
            when {
                hold() -> 0f
                travelling -> offset - lead
                else -> scrollDistance(offset, size, containerSize, lead, trail()).also { travelling = it != 0f }
            }
        }
}

/** The scroll distance for a child at [offset] of [size] in a container: see [AnchoredSpec]. */
internal fun scrollDistance(
    offset: Float,
    size: Float,
    containerSize: Float,
    lead: Float,
    trail: Float,
): Float = if (offset < lead || offset + size + trail > containerSize) offset - lead else 0f

/** Applies [AnchoredSpec] to [list], the page inside [content]. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AnchoredScrolling(
    lead: Dp,
    trail: Dp,
    list: ScrollableState,
    hold: () -> Boolean = { false },
    content: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val leadPx by rememberUpdatedState(with(density) { lead.toPx() })
    val trailPx by rememberUpdatedState(with(density) { trail.toPx() })
    val holding by rememberUpdatedState(hold)
    // One instance for the list: a new one mid-scroll (the lead changes as rows report their header) would
    // forget the scroll it is carrying and stop it halfway again.
    val spec = remember(list) { AnchoredSpec(list, lead = { leadPx }, trail = { trailPx }, hold = { holding() }) }
    CompositionLocalProvider(LocalBringIntoViewSpec provides spec, content = content)
}

/**
 * A lane's scroll ([RoomSpec]), as the web's rows: a focused tile comes into view with the least movement,
 * [room] clear of either end, so the next tile peeks past it (plans/m6-android.md 7.3, rules 5 and 8).
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun LaneScrolling(
    room: Dp,
    content: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val spec = remember(density, room) { RoomSpec(with(density) { room.toPx() }) }
    CompositionLocalProvider(LocalBringIntoViewSpec provides spec, content = content)
}
