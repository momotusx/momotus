package app.marquee.design

import android.view.accessibility.AccessibilityManager
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlin.math.abs

/**
 * Home's hero carousel as state (plans/appletv-polish.md 4.1): one progress value drives both the active dot's
 * fill and the advance, so a pause or reduced motion can never make them drift. The page shown is the pager's;
 * this holds the clock and whether the viewer has taken over.
 */
class HeroClock(
    count: Int,
    private val dwellMs: Long = DWELL_MS,
) {
    var count by mutableIntStateOf(count)

    /**
     * 0 to 1 through the page's dwell, or through its trailer while one plays. Written once per 1 percent, as the
     * web does: the dot that draws it redraws about every 80 ms instead of every frame, the same to the eye on a
     * 44 dp pill (review r3, M1: the TV clock now runs while Home sits idle).
     */
    var progress by mutableFloatStateOf(0f)
        private set

    /** The dwell's exact place, which [progress] follows by whole percents. */
    private var exact = 0f

    private fun set(value: Float) {
        exact = value
        progress = value
    }

    /** Paged by hand: auto-advance stops for the session. */
    var stopped by mutableStateOf(false)
        private set

    /** Advances the clock by [dtMs] while auto-advance may run; true when the page should change now. */
    fun tick(
        dtMs: Long,
        running: Boolean,
    ): Boolean {
        if (!running || stopped || count < 2) return false
        exact = (exact + dtMs.toFloat() / dwellMs).coerceAtMost(1f)
        if (exact >= 1f) {
            set(0f)
            return true
        }
        // A hair under a percent, so float steps of 0.002 still land on every fifth frame.
        if (exact - progress >= STEP - 1e-4f) progress = exact
        return false
    }

    /** A trailer's place in its cap, written to the same value. */
    fun trailer(fraction: Float) {
        set(fraction.coerceIn(0f, 1f))
    }

    /** Any touch or key: the dwell starts over. */
    fun input() {
        set(0f)
    }

    /** The viewer paged: no more timed pages this session. */
    fun manual() {
        stopped = true
        set(0f)
    }

    /** A page change from the timer: the next dwell starts. */
    fun paged() {
        set(0f)
    }

    companion object {
        // The owner's B2 (design/tokens.json motion): a page stays 8 s and slides in 650 ms.
        const val DWELL_MS = Tokens.HERO_DWELL_MS
        const val SLIDE_MS = Tokens.HERO_SLIDE_MS

        /** The smallest change of [progress] that reaches the dot. */
        const val STEP = 0.01f

        /** How often a running clock wakes: one [STEP] of the dwell. */
        const val TICK_MS = (DWELL_MS * STEP).toLong()
    }
}

/**
 * Whether the dwell may run (decision 7): never while a trailer plays, so the 8 s dwell cannot cut it; that page
 * moves on by the trailer's end rule instead (the lead, 2026-10-24).
 */
fun heroClockRuns(
    active: Boolean,
    visible: Boolean,
    reducedMotion: Boolean,
    sliding: Boolean,
    trailerPlaying: Boolean,
    pages: Int,
    /** TalkBack is on: a page that slides away under its focus is lost to it (WCAG 2.2.2; review r1, L4). */
    screenReader: Boolean = false,
    /** TV: a key went down in the last 5 s, so nothing slides while the viewer moves (the owner, 2026-10-26). */
    held: Boolean = false,
): Boolean = pages > 1 && active && visible && !reducedMotion && !sliding && !trailerPlaying && !screenReader && !held

/** Whether TalkBack (touch exploration) is on, following it as it changes. */
@Composable
fun rememberScreenReader(): Boolean {
    val context = LocalContext.current
    val manager = remember(context) { context.getSystemService(AccessibilityManager::class.java) }
    var on by remember(manager) { mutableStateOf(manager?.isTouchExplorationEnabled == true) }
    DisposableEffect(manager) {
        val listener = AccessibilityManager.TouchExplorationStateChangeListener { on = it }
        manager?.addTouchExplorationStateChangeListener(listener)
        onDispose { manager?.removeTouchExplorationStateChangeListener(listener) }
    }
    return on
}

/**
 * Where a hero page sits during a slide, in page widths from its place at rest (B2: the two pages move together,
 * edge to edge): the page coming in starts a page away on the side it comes from, the one going out ends a page
 * away on the other. [dir] is 1 for the next page, -1 for the one before; [t] runs 0 to 1.
 */
fun heroPageAt(
    incoming: Boolean,
    dir: Int,
    t: Float,
): Float = if (incoming) dir * (1f - t) else -dir * t

/** The phone pager lays the core's pages out [HERO_LOOPS] times over, so the timer's next page is always to the
 * right, the last page's too (review r1, L3); a page's place in the core's list is `page % n`. */
const val HERO_LOOPS = 1000

/** The pager's page count for [n] pages. */
fun heroPagerCount(n: Int): Int = if (n > 1) n * HERO_LOOPS else n

/** The pager page that shows the core's page [index] in the middle loop. */
fun heroPagerPage(
    n: Int,
    index: Int,
): Int = if (n > 1) n * (HERO_LOOPS / 2) + index else 0

/** The page nearest [current] that shows the core's page [index], for a tap on a dot. */
fun heroNearestPage(
    n: Int,
    current: Int,
    index: Int,
): Int = if (n > 1) current - current % n + index else 0

/** A drag by hand on [source] stops the timed pages for the visit: the phone's swipe (review r2, S3). */
suspend fun HeroClock.stopOnDrag(source: InteractionSource) {
    source.interactions.collect { if (it is DragInteraction.Start) manual() }
}

/**
 * Runs [clock] on the frame clock while [running] says so, and calls [onAdvance] when a page's time is up.
 * Compose's frame clock is the test clock in Robolectric, so the dwell is testable without real time.
 */
@Composable
fun RunHeroClock(
    clock: HeroClock,
    /** Read here, in this function's own scope: a change of focus or scroll recomposes it, not the caller. */
    running: () -> Boolean,
    onAdvance: () -> Unit,
) {
    val advance by rememberUpdatedState(onAdvance)
    val on = running()
    // Frames are asked for only while the clock runs: an idle hero asks for none (and a test can settle). It wakes once
    // per step of the dot (8 s over 100 steps), not on every vsync: the dot moves by whole steps anyway, and a page
    // turns at most a step late. The frame clock still times each step.
    LaunchedEffect(clock, on) {
        if (!on) return@LaunchedEffect
        var last = withFrameNanos { it }
        while (true) {
            delay(HeroClock.TICK_MS)
            withFrameNanos { now ->
                val dt = (now - last) / 1_000_000
                last += dt * 1_000_000
                if (clock.tick(dt, true)) advance()
            }
        }
    }
}

/**
 * The page dots (the iPhone TV app's): dots on a fixed pitch, the active one a capsule whose white fill grows over
 * the page's time. Phone 7 on a 16 pitch with a 24 pill; TV 12 on 28 with a 44 pill, not focusable there.
 * With [onPick] the row is one control, as iOS's page control: at least 48 by 48 dp, a tap goes to the dot nearest
 * it, and a screen reader hears the page shown with Previous page and Next page as its actions. A dot of its own
 * would be a 16 dp wide target.
 */
@Composable
fun HeroDots(
    count: Int,
    active: Int,
    progress: () -> Float,
    modifier: Modifier = Modifier,
    tv: Boolean = false,
    label: (Int) -> String = { "Page ${it + 1} of $count" },
    onPick: ((Int) -> Unit)? = null,
) {
    val dot = if (tv) 12.dp else 7.dp
    val pill = if (tv) 44.dp else 24.dp
    val gap = if (tv) 16.dp else 9.dp
    val pick by rememberUpdatedState(onPick)
    val control =
        if (onPick == null) {
            Modifier
        } else {
            Modifier
                .sizeIn(minWidth = 48.dp, minHeight = 48.dp)
                .pointerInput(count, active) {
                    detectTapGestures { at ->
                        val x = at.x.toDp().value
                        pick?.invoke(
                            nearestDot(x, size.width.toDp().value, count, active, dot.value, pill.value, gap.value),
                        )
                    }
                }.semantics {
                    contentDescription = label(active)
                    customActions =
                        listOfNotNull(
                            CustomAccessibilityAction("Previous page") {
                                onPick(active - 1)
                                true
                            }.takeIf { active > 0 },
                            CustomAccessibilityAction("Next page") {
                                onPick(active + 1)
                                true
                            }.takeIf { active < count - 1 },
                        )
                }
        }
    Row(
        modifier.then(control),
        horizontalArrangement = Arrangement.spacedBy(gap, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        repeat(count) { i -> Dot(i == active, dot, pill, progress) }
    }
}

/**
 * The dot a tap [x] dp from the row's start is nearest to, in a row [width] dp wide whose dots sit in its middle: dots
 * [dot] wide, the active one [pill], [gap] apart.
 */
internal fun nearestDot(
    x: Float,
    width: Float,
    count: Int,
    active: Int,
    dot: Float,
    pill: Float,
    gap: Float,
): Int {
    var left = maxOf(0f, (width - (pill + (count - 1) * (dot + gap))) / 2)
    return (0 until count).minByOrNull { i ->
        val w = if (i == active) pill else dot
        abs(left + w / 2 - x).also { left += w + gap }
    } ?: 0
}

@Composable
private fun Dot(
    on: Boolean,
    size: Dp,
    pill: Dp,
    progress: () -> Float,
) {
    val ms = Marquee.panelMs
    val width by animateDpAsState(if (on) pill else size, tween(ms), label = "dot")
    val track by animateColorAsState(if (on) DOT_TRACK else DOT, tween(ms), label = "dotColor")
    // The fill is drawn, not laid out: the clock moves it every frame without recomposing.
    Box(
        Modifier
            .width(width)
            .height(size)
            .clip(RoundedCornerShape(50))
            .background(track)
            .drawBehind {
                if (on) {
                    drawRoundRect(
                        Color.White,
                        size = Size(this.size.width * progress(), this.size.height),
                        cornerRadius = CornerRadius(this.size.height / 2),
                    )
                }
            },
    )
}

/** The inactive dot: white at 60 percent; the pill's track: white at 35 percent. */
private val DOT = Color.White.copy(alpha = 0.6f)
private val DOT_TRACK = Color.White.copy(alpha = 0.35f)
