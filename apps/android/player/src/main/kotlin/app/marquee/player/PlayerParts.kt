package app.marquee.player

import android.view.accessibility.AccessibilityManager
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.TraktCopy
import app.marquee.core.model.Attempt
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.OtherVersion
import app.marquee.core.model.Segment
import app.marquee.core.model.SubtitleSize
import app.marquee.core.model.TraktPlayback
import app.marquee.design.CapsLabel
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PrimaryButton
import app.marquee.design.RemoteImage
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.TextRole
import app.marquee.design.endFade
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus
import app.marquee.design.scrollByKey
import app.marquee.design.windowDp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** 3725 s as "1:02:05". */
fun clockText(ms: Long): String = Format.clock(ms / 1000)

/** The time of day the film ends at the current speed, or null while the length is unknown. */
fun endsAt(
    nowMs: Long,
    positionMs: Long,
    durationMs: Long,
    speed: Float,
): Long? =
    if (durationMs <=
        0
    ) {
        null
    } else {
        nowMs + ((durationMs - positionMs).coerceAtLeast(0) / speed.coerceAtLeast(0.1f)).toLong()
    }

/**
 * The position bar: what is buffered, what is played, and a knob. The values are read while
 * drawing, so a tick of the clock redraws this bar and recomposes nothing.
 *
 * @param preview where a D-pad scrub or a drag would land, drawn instead of the position.
 * @param emphasized focus is on the bar: the knob grows.
 */
@Composable
fun Scrubber(
    position: () -> Long,
    duration: () -> Long,
    buffered: () -> Long,
    modifier: Modifier = Modifier,
    preview: () -> Long? = { null },
    emphasized: Boolean = false,
    /** What a screen reader's "set progress" does; null leaves the bar read-only to it. */
    onSeek: ((Long) -> Unit)? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val track = if (tv) 8.dp else 4.dp
    val knob = (if (tv) 11.dp else 7.dp) * (if (emphasized) 1.4f else 1f)
    Canvas(
        modifier
            .height(knob * 2)
            .semantics {
                // Read when a screen reader asks, so the clock does not recompose this.
                contentDescription = "Position"
                stateDescription = "${clockText(position())} of ${clockText(duration())}"
                progressBarRangeInfo =
                    ProgressBarRangeInfo(position().toFloat(), 0f..duration().coerceAtLeast(1L).toFloat())
                if (onSeek != null) {
                    setProgress {
                        onSeek(it.toLong())
                        true
                    }
                }
            },
    ) {
        val total = duration().coerceAtLeast(1L).toFloat()

        fun at(ms: Long) = size.width * (ms / total).coerceIn(0f, 1f)
        val y = (size.height - track.toPx()) / 2
        val corner =
            androidx.compose.ui.geometry
                .CornerRadius(track.toPx() / 2)
        val bar = Size(size.width, track.toPx())
        drawRoundRect(colors.text.copy(alpha = 0.25f), Offset(0f, y), bar, corner)
        drawRoundRect(colors.text.copy(alpha = 0.42f), Offset(0f, y), Size(at(buffered()), track.toPx()), corner)
        val here = preview() ?: position()
        drawRoundRect(colors.text, Offset(0f, y), Size(at(here), track.toPx()), corner)
        drawCircle(colors.text, knob.toPx(), Offset(at(here), size.height / 2))
    }
}

/** The audio list: one row per track with its full name and what it is under it, the chosen one filled and checked. On a TV focus starts on the chosen row. */
@Composable
fun TrackPanel(
    title: String,
    rows: List<TrackChoice>,
    onPick: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val requesters = remember(rows.size) { List(rows.size) { FocusRequester() } }
    val chosen = rows.indexOfFirst { it.selected }.coerceAtLeast(0)
    // The list starts at the chosen row, so its node exists to take focus.
    val state = rememberLazyListState(initialFirstVisibleItemIndex = (chosen - 1).coerceAtLeast(0))
    if (tv) {
        LaunchedEffect(Unit) {
            withFrameNanos { }
            withFrameNanos { }
            runCatching { requesters.getOrNull(chosen)?.requestFocus() }
        }
    }
    Column(
        modifier
            .then(if (tv) Modifier.width(640.dp) else Modifier)
            .clip(RoundedCornerShape(Marquee.radius.l))
            .background(colors.surface)
            .border(2.dp, colors.line, RoundedCornerShape(Marquee.radius.l))
            .padding(vertical = if (tv) 24.dp else 12.dp, horizontal = if (tv) 20.dp else 8.dp)
            // The panel is its own pane for a screen reader, with its name.
            .semantics { paneTitle = title },
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqText(
            title,
            role = TextRole.Headline,
            modifier = Modifier.padding(horizontal = 12.dp).semantics { heading() },
            maxLines = 1,
        )
        LazyColumn(
            Modifier.heightIn(max = if (tv) 520.dp else 230.dp),
            state = state,
            // Room for the focus ring and its halo, which the list would otherwise clip at both ends.
            contentPadding = PaddingValues(vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            rows.forEachIndexed { index, row ->
                item(key = row.id) { TrackRow(row, index, rows.lastIndex, requesters[index], onPick) }
            }
        }
    }
}

/**
 * The chosen track's subtitle timing, under a language's options, as Stremio's delay: minus and plus around the delay
 * ("+0.3 s"), a tenth of a second a press, faster while held ([DelayHold]); Reset, or a press on the delay itself,
 * puts it back to 0. On a TV the delay takes the focus: Left and Right step it the same way, select resets, and Down
 * reaches Reset. The picker stays open.
 */
@Composable
fun TimingRow(
    delayMs: Long,
    onDelay: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    val delay = rememberUpdatedState(delayMs)
    val set = rememberUpdatedState(onDelay)
    val scope = rememberCoroutineScope()
    val hold = remember { DelayHold(scope, delay, set) }
    val reset = { if (delay.value != 0L) set.value(0L) }
    val resetFocus = remember { FocusRequester() }
    val valueFocus = remember { FocusRequester() }
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 14.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText("Timing", Modifier.weight(1f), role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
        if (tv) {
            MqIcon(MINUS, tint = Marquee.colors.dim, size = 26.dp)
            DelayValue(
                delayMs,
                reset,
                Modifier
                    .onPreviewKeyEvent { delayKey(it, hold) }
                    .onFocusChanged { if (!it.isFocused) hold.release(step = false) }
                    .focusRequester(valueFocus)
                    .focusProperties { if (delayMs != 0L) down = resetFocus },
            )
            MqIcon(Icons.Plus, tint = Marquee.colors.dim, size = 26.dp)
        } else {
            HoldButton(MINUS, "Minus 0.1 seconds", -1, hold)
            DelayValue(delayMs, reset)
            HoldButton(Icons.Plus, "Plus 0.1 seconds", 1, hold)
        }
        if (delayMs != 0L) {
            SecondaryButton(
                "Reset",
                {
                    // Reset leaves with the press: on a TV the focus goes back to the delay.
                    if (tv) runCatching { valueFocus.requestFocus() }
                    reset()
                },
                Modifier.focusRequester(resetFocus).focusProperties { right = FocusRequester.Cancel },
            )
        }
    }
}

/**
 * Minus or plus held, or a TV's Left or Right on the delay, as stremio-web's: a release before the first repeat is one
 * press; held, it steps after [HOLD_START_MS] and every [HOLD_EVERY_MS] after that, faster as it goes ([steppedDelay]).
 * The hold counts its own time, so a slow frame does not speed it up.
 */
private class DelayHold(
    private val scope: CoroutineScope,
    private val delayMs: State<Long>,
    private val onDelay: State<(Long) -> Unit>,
) {
    private var job: Job? = null
    private var direction = 0
    private var repeated = false

    fun press(direction: Int) {
        if (job != null) return
        this.direction = direction
        repeated = false
        job =
            scope.launch {
                // As stremio-web's Stepper: the repeat interval starts after the hold's wait, so the first tick
                // comes one interval later.
                delay(HOLD_START_MS + HOLD_EVERY_MS)
                repeated = true
                var at = delayMs.value
                var held = HOLD_START_MS + HOLD_EVERY_MS
                while (true) {
                    at = steppedDelay(at, direction, held)
                    onDelay.value(at)
                    delay(HOLD_EVERY_MS)
                    held += HOLD_EVERY_MS
                }
            }
    }

    /** Let go: one step if it never repeated; none when the touch was cancelled or the focus left ([step] false). */
    fun release(step: Boolean = true) {
        val held = job ?: return
        held.cancel()
        job = null
        if (step && !repeated) onDelay.value(steppedDelay(delayMs.value, direction))
    }
}

/** Left and Right on the TV's focused delay, held or pressed ([DelayHold]); the rest goes on. */
private fun delayKey(
    event: KeyEvent,
    hold: DelayHold,
): Boolean {
    val direction =
        when (event.key) {
            Key.DirectionLeft -> -1
            Key.DirectionRight -> 1
            else -> return false
        }
    // The system's own repeats of a held key are ignored: the hold keeps its own pace.
    when (event.type) {
        KeyEventType.KeyDown -> hold.press(direction)
        KeyEventType.KeyUp -> hold.release()
    }
    return true
}

/** The delay ("+0.3 s"); a press resets it to 0. Read out in words ("Plus 0.3 seconds"), as it changes. */
@Composable
private fun DelayValue(
    delayMs: Long,
    onReset: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val ink = focusPalette(focused)
    Box(
        modifier
            .onFocusChanged { focused = it.isFocused }
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(if (focused && tv) ink.primary else Color.Transparent)
            .focusClickable(onReset)
            .semantics {
                contentDescription = "Timing"
                stateDescription = timingWords(delayMs)
                liveRegion = LiveRegionMode.Polite
            }.widthIn(min = if (tv) 120.dp else 64.dp)
            .padding(horizontal = if (tv) 16.dp else 6.dp, vertical = if (tv) 10.dp else 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        MqText(
            delayText(delayMs),
            role = TextRole.Callout,
            color = ink.text,
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

/** A round minus or plus that steps [direction] on a press and faster while held ([DelayHold]). A screen reader presses it once. */
@Composable
private fun HoldButton(
    icon: ImageVector,
    spoken: String,
    direction: Int,
    hold: DelayHold,
) {
    Box(
        Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(SECONDARY)
            .semantics {
                contentDescription = spoken
                role = Role.Button
                onClick {
                    hold.press(direction)
                    hold.release()
                    true
                }
            }.pointerInput(Unit) {
                detectTapGestures(
                    onPress = {
                        hold.press(direction)
                        hold.release(step = tryAwaitRelease())
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) { MqIcon(icon, tint = Color.White, size = 20.dp) }
}

/** A secondary control's fill over the picture: white at 16 percent (design/DECISION.md, Secondary actions). */
private val SECONDARY = Color(0x29FFFFFF)

/** Lucide's "minus", which the design module's set does not carry. */
private val MINUS: ImageVector by lazy {
    ImageVector
        .Builder("minus", 24.dp, 24.dp, 24f, 24f)
        .addPath(
            addPathNodes("M5 12h14"),
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 2f,
            strokeLineCap = StrokeCap.Round,
        ).build()
}

/**
 * The profile's subtitle size, under the timing: Smaller and Larger step through its four sizes, which hold for the
 * next videos too (`subtitleSize`, as Settings sets it); at either end the press changes nothing.
 */
@Composable
fun SizeRow(
    size: SubtitleSize,
    onSize: (SubtitleSize) -> Unit,
    modifier: Modifier = Modifier,
    first: Modifier = Modifier,
) = StepRow(
    "Size",
    sizeWords(size),
    "Smaller",
    { stepSize(size, -1)?.let(onSize) },
    "Larger",
    { stepSize(size, 1)?.let(onSize) },
    modifier,
    first,
)

/** A setting the picker steps with two buttons: its name and value at the left, the buttons at the right. */
@Composable
private fun StepRow(
    title: String,
    value: String,
    less: String,
    onLess: () -> Unit,
    more: String,
    onMore: () -> Unit,
    modifier: Modifier,
    first: Modifier,
) {
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            MqText(title, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
            MqText(
                value,
                role = TextRole.Caption,
                color = Marquee.colors.muted,
                maxLines = 1,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
        SecondaryButton(less, onLess, first)
        SecondaryButton(more, onMore, Modifier.focusProperties { right = FocusRequester.Cancel })
    }
}

/** The profile's subtitle sizes in order, as the picker steps them. */
private val SIZES = listOf(SubtitleSize.Small, SubtitleSize.Medium, SubtitleSize.Large, SubtitleSize.ExtraLarge)

/** The size [by] steps from [size], or null past either end; a size from a newer core steps from Medium. */
fun stepSize(
    size: SubtitleSize,
    by: Int,
): SubtitleSize? = SIZES.getOrNull(SIZES.indexOf(size).let { if (it < 0) 1 else it } + by)

/** "100%": how big this app draws [size] ([subtitleScale]). */
fun sizeWords(size: SubtitleSize): String = "${(subtitleScale(size) * 100).roundToInt()}%"

/** One track: its name and what it is, the chosen one filled and checked; focus stays in the panel. */
@Composable
private fun TrackRow(
    row: TrackChoice,
    index: Int,
    last: Int,
    requester: FocusRequester,
    onPick: (String) -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.m)
    // The chosen row is a filled row with a check; the ring is for focus alone.
    Row(
        Modifier
            .fillMaxWidth()
            .focusRequester(requester)
            // Focus stays in the panel while it is open: the ends of the list and the sides go nowhere.
            .focusProperties {
                left = FocusRequester.Cancel
                right = FocusRequester.Cancel
                if (index == 0) up = FocusRequester.Cancel
                if (index == last) down = FocusRequester.Cancel
            }.marqueeFocus(Marquee.radius.m, scale = 1.02f)
            .clip(shape)
            .background(if (row.selected) colors.raised else Color.Transparent)
            .semantics {
                selected = row.selected
                contentDescription = listOfNotNull(row.name, row.detail).joinToString(", ")
            }.focusClickable({ onPick(row.id) }, Role.RadioButton)
            .heightIn(min = 48.dp)
            .padding(horizontal = 16.dp, vertical = if (tv) 12.dp else 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(if (tv) 30.dp else 18.dp)) {
            if (row.selected) MqIcon(Icons.Check, tint = colors.text)
        }
        Column {
            MqText(row.name, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
            row.detail?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
        }
    }
}

/**
 * What is happening before anything plays, or why nothing does: a turning mark and one line, or the
 * problem with one plain reason per source that failed, and the way on. A [notice] is said without the mark.
 */
@Composable
fun PlayerStatus(
    stage: PlaybackController.Stage,
    status: String?,
    notice: String?,
    problem: String?,
    attempts: List<Attempt>,
    onVersions: () -> Unit,
    onBack: () -> Unit,
    focus: FocusRequester,
    modifier: Modifier = Modifier,
    /** The core's next version after a version picked by hand failed: the first way on, "Play {label}". */
    offer: OtherVersion? = null,
    onOffer: (String) -> Unit = {},
    /** "Try again": after Play {offer}, after Choose a version when the version could not be opened, else first. */
    onRetry: (() -> Unit)? = null,
    /** The core took a Try again and the play failed again at once: "Trying again" under the headline, a moment. */
    tryingAgain: Boolean = false,
    /** Why the version could not be opened, under the headline. */
    detail: String? = null,
    /** The core could not open the version: Choose a version comes first, Try again second. */
    cannotOpen: Boolean = false,
    /** Why nothing played, as the core names it ([PlaybackController.empty]): the shared table's actions. */
    empty: EmptyKind? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        if (stage == PlaybackController.Stage.Failed) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MqIcon(Icons.CircleAlert, tint = colors.bad, size = if (tv) 36.dp else 22.dp)
                MqText(problem ?: "Playback stopped", role = TextRole.Headline, weight = FontWeight.Medium)
            }
            detail?.let { MqText(it, role = TextRole.Callout, color = colors.muted) }
            if (tryingAgain) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Spinner(size = if (tv) 22.dp else 16.dp)
                    MqText(TRYING_AGAIN, role = TextRole.Callout, color = colors.muted)
                }
            }
            // The history takes what is left above the recovery row and scrolls past that (on TV with
            // Up and Down once it overflows), so many long failures never push the way out off screen.
            val history = rememberScrollState()
            val scope = rememberCoroutineScope()
            val overflows = history.maxValue in 1 until Int.MAX_VALUE
            Column(
                Modifier
                    .weight(1f, fill = false)
                    .then(
                        if (tv && overflows) {
                            Modifier
                                .onPreviewKeyEvent { scrollByKey(it, history, scope) }
                                .marqueeFocus(Marquee.radius.m, scale = 1f)
                                .focusable()
                        } else {
                            Modifier
                        },
                    ).endFade(history.canScrollForward, vertical = true)
                    .verticalScroll(history),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                for (failed in groupFailures(attempts)) {
                    // One spoken line per row: the addon, then why.
                    Row(
                        Modifier.semantics(mergeDescendants = true) {},
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    ) {
                        MqText(failed.via, role = TextRole.Callout, weight = FontWeight.SemiBold)
                        // The core words the reason in plain sentences ("The link no longer exists (404)").
                        MqText(failed.words, role = TextRole.Callout, color = colors.muted)
                    }
                }
            }
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
                modifier = Modifier.padding(top = Marquee.space.xs),
            ) {
                // The first one takes focus (plans/versions-android-spec.md, section 6; the shared table).
                var backFirst = false
                if (offer != null) {
                    // A hand pick did not start: the core's next version, another version, Back.
                    PrimaryButton(
                        "Play ${offer.label} instead",
                        { onOffer(offer.key) },
                        icon = PlayFilled,
                        focusRequester = focus,
                    )
                    SecondaryButton(CHOOSE_ANOTHER, onVersions, icon = Icons.Layers)
                } else if (empty == EmptyKind.NotStarted && onRetry != null) {
                    // The start's 90 s ran out: Try again, another version, Back.
                    PrimaryButton(
                        StreamChoiceCopy.EMPTY_TRY_AGAIN,
                        onRetry,
                        icon = Icons.RefreshCw,
                        focusRequester = focus,
                    )
                    SecondaryButton(CHOOSE_ANOTHER, onVersions, icon = Icons.Layers)
                } else if (empty == EmptyKind.NoRoom) {
                    // No room here: nothing but Back.
                    backFirst = true
                } else if (empty == EmptyKind.Unreachable && onRetry != null) {
                    PrimaryButton(
                        StreamChoiceCopy.EMPTY_TRY_AGAIN,
                        onRetry,
                        icon = Icons.RefreshCw,
                        focusRequester = focus,
                    )
                } else if (onRetry != null && cannotOpen) {
                    // A version the core could not open is mostly one no longer listed, which Try again opens
                    // to the same answer: another version is the way on, and Try again (it helps after a
                    // reconnect) comes second (review 2026-10-20 r4 P3-6, as on the web and Apple).
                    PrimaryButton("Choose a version", onVersions, icon = Icons.Layers, focusRequester = focus)
                    SecondaryButton("Try again", onRetry, icon = Icons.RefreshCw)
                } else if (onRetry != null) {
                    PrimaryButton("Try again", onRetry, icon = Icons.RefreshCw, focusRequester = focus)
                    SecondaryButton("Choose a version", onVersions, icon = Icons.Layers)
                } else {
                    PrimaryButton("Choose a version", onVersions, icon = Icons.Layers, focusRequester = focus)
                }
                if (backFirst) {
                    PrimaryButton(StreamChoiceCopy.BACK, onBack, icon = Icons.ArrowLeft, focusRequester = focus)
                } else {
                    SecondaryButton(StreamChoiceCopy.BACK, onBack)
                }
            }
        } else {
            if (status != null) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Spinner()
                    MqText(status, role = TextRole.Headline, weight = FontWeight.Medium)
                }
            }
            if (notice != null) MqText(notice, role = TextRole.Headline, weight = FontWeight.Medium)
        }
    }
}

/** The card that offers the next episode near the end: its picture, name, and a countdown when it starts by itself. */
@Composable
fun UpNextCard(
    next: NextEpisode,
    countdown: Int?,
    onPlay: () -> Unit,
    modifier: Modifier = Modifier,
    focus: FocusRequester? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.l)
    Row(
        modifier
            .clip(shape)
            .background(colors.surface)
            .border(2.dp, colors.line, shape)
            .padding(if (tv) 24.dp else 12.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 12.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        // A short landscape phone keeps the card to its words and button.
        if (tv || windowDp().height >= SHORT_PHONE_DP.dp) {
            RemoteImage(
                url = next.thumbnail,
                width = if (tv) 320.dp else 120.dp,
                height = if (tv) 180.dp else 68.dp,
                fallbackText = next.title,
                modifier = Modifier.clip(RoundedCornerShape(Marquee.radius.m)),
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            MqText("Next episode", role = TextRole.Caption, color = colors.muted)
            MqText(
                "${Format.episodeLabel(next.season, next.episode)} ${next.title}".trim(),
                role = TextRole.Headline,
                maxLines = 2,
                modifier = Modifier.widthIn(max = if (tv) 420.dp else 200.dp),
            )
            if (countdown != null) MqText("Starts in $countdown s", role = TextRole.Callout, color = colors.muted)
            PrimaryButton("Play now", onPlay, icon = Icons.SkipForward, focusRequester = focus)
        }
    }
}

/**
 * The button over the picture while a skippable part plays ("Skip intro"), with where the timing came from
 * above it ("Skip data: TheIntroDB"). It seeks only when pressed, never by itself.
 */
@Composable
fun SkipButton(
    segment: Segment,
    onSkip: () -> Unit,
    modifier: Modifier = Modifier,
    focus: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
) {
    Column(
        modifier,
        horizontalAlignment = Alignment.End,
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqText("Skip data: ${segment.source}", role = TextRole.Caption, color = Marquee.colors.muted, maxLines = 1)
        // A plain white pill: an accent edge that never goes away reads as focus stuck on it.
        PrimaryButton(
            skipLabel(segment.kind),
            onSkip,
            icon = Icons.SkipForward,
            focusRequester = focus,
            onFocusChange = onFocusChange,
        )
    }
}

/**
 * What the player says when it reached the end: over the last frame, the way on. A series offers
 * the next episode first, then "Play again" and "Back to <title>"; a movie puts "Back to <title>"
 * first. The scrim is full-bleed, so the picture is still there behind the words.
 */
@Composable
fun EndCard(
    title: String,
    next: NextEpisode?,
    onNext: () -> Unit,
    onAgain: () -> Unit,
    onBack: () -> Unit,
    focus: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Box(modifier.background(Color.Black.copy(alpha = 0.72f)), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
            // What ended, as on the web: a capitals kicker over the name.
            CapsLabel("Finished", color = colors.muted)
            MqText(title, role = TextRole.Title1, maxLines = 2, textAlign = TextAlign.Center)
            if (next != null) {
                MqText(
                    "Next: ${Format.episodeLabel(next.season, next.episode)} ${next.title}".trim(),
                    role = TextRole.Callout,
                    color = colors.muted,
                    maxLines = 2,
                )
            }
            val buttons: List<Triple<String, () -> Unit, ImageVector?>> =
                buildList {
                    if (next != null) add(Triple("Next episode", onNext, Icons.SkipForward))
                    if (next != null) add(Triple("Play again", onAgain, Icons.RotateCcw))
                    add(Triple("Back to $title", onBack, Icons.ArrowLeft))
                    if (next == null) add(Triple("Play again", onAgain, Icons.RotateCcw))
                }
            val buttonRow: @Composable () -> Unit = {
                buttons.forEachIndexed { i, (label, action, icon) ->
                    if (i == 0) {
                        PrimaryButton(label, action, icon = icon, focusRequester = focus)
                    } else {
                        SecondaryButton(label, action, icon = icon)
                    }
                }
            }
            if (tv) {
                Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) { buttonRow() }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) { buttonRow() }
            }
        }
    }
}

/**
 * The stops of a scrim as (position, black alpha) from the top of the scrim to its bottom. A scrim
 * is not a straight ramp: it reaches 80% of its [peak] a quarter of the way in from the far end
 * (the foot) or 70% of it 70% of the way down (the top), so the words that sit well away from the screen
 * edge (the position bar) are already on a dark ground, and a white picture behind them still gives
 * 4.5:1 (measured by scripts/player-contrast.ts).
 */
fun scrimStops(
    top: Boolean,
    peak: Float,
): List<Pair<Float, Float>> =
    if (top) {
        listOf(0f to peak, 0.7f to peak * 0.7f, 1f to 0f)
    } else {
        listOf(0f to 0f, 0.25f to peak * 0.8f, 1f to peak)
    }

/** The black alpha of a scrim [at] (0 at its top, 1 at its bottom), between its stops. */
fun scrimAlpha(
    top: Boolean,
    peak: Float,
    at: Float,
): Float {
    val stops = scrimStops(top, peak)
    val x = at.coerceIn(0f, 1f)
    val upper = stops.indexOfFirst { it.first >= x }.coerceAtLeast(1)
    val (x0, a0) = stops[upper - 1]
    val (x1, a1) = stops[upper]
    return a0 + (a1 - a0) * ((x - x0) / (x1 - x0))
}

/** A scrim from one edge, full width, so there is no box edge to see; [peak] is the black alpha at the edge. */
@Composable
fun Scrim(
    top: Boolean,
    height: Dp,
    peak: Float,
    modifier: Modifier = Modifier,
) {
    val colors = scrimStops(top, peak).map { (at, alpha) -> at to Color.Black.copy(alpha = alpha) }.toTypedArray()
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .background(Brush.verticalGradient(colorStops = colors)),
    )
}

/**
 * An action of the OSD: icon, bold name, and a small line under it (what is chosen now). [busy] puts a small
 * turning mark beside that line while more is still on the way (subtitles from slower sources).
 */
@Composable
fun ActionButton(
    icon: ImageVector,
    label: String,
    detail: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
    busy: Boolean = false,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Column(
        modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(Marquee.radius.m, scale = 1.04f)
            .clip(RoundedCornerShape(Marquee.radius.m))
            .semantics {
                contentDescription = listOfNotNull(label, detail, BUSY_WORDS.takeIf { busy }).joinToString(", ")
            }.focusClickable(onClick, Role.Button)
            // Wide enough to touch (48 dp on a phone), and short enough that five fit beside the transport on TV.
            .widthIn(min = if (tv) 128.dp else 48.dp, max = if (tv) 190.dp else 150.dp)
            // Side padding keeps neighbours apart when a phone's row is full and scrolls.
            .padding(horizontal = if (tv) 8.dp else 6.dp, vertical = if (tv) 10.dp else 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        MqIcon(icon, tint = colors.text, size = if (tv) 32.dp else 20.dp)
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
        detail?.let {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 1)
                if (busy) Spinner(Modifier.testTag(BUSY_TAG), size = if (tv) 14.dp else 10.dp)
            }
        }
    }
}

/** Read out after an action's line while more is on the way. */
const val BUSY_WORDS = "still searching"

/** The turning mark beside an action's line, for tests. */
const val BUSY_TAG = "action-busy"

private const val SHORT_PHONE_DP = 420

/** 1.0 as "1x", 1.25 as "1.25x". */
fun speedText(speed: Float): String = if (speed % 1f == 0f) "${speed.toInt()}x" else "${speed}x"

/** The speeds the speed button steps through, back to 1x after the last. */
val SPEEDS = listOf(1f, 1.25f, 1.5f, 2f, 0.75f)

fun nextSpeed(current: Float): Float = SPEEDS[(SPEEDS.indexOf(current) + 1) % SPEEDS.size]

/**
 * Trakt's side of this play when it is recorded (`PlaybackSnapshot.trakt` on): a small mark with the
 * plain word "Trakt" (no logo) in the title block, read out as a sentence. The states where the play is
 * not recorded say so in [TraktWarning], which stays when the controls hide.
 */
@Composable
fun TraktMark(
    trakt: TraktPlayback?,
    modifier: Modifier = Modifier,
) {
    if (trakt != TraktPlayback.On) return
    val spoken = TraktCopy.player(trakt) ?: return
    val tv = Marquee.form == Form.Tv
    Row(
        modifier.clearAndSetSemantics { contentDescription = spoken },
        horizontalArrangement = Arrangement.spacedBy(if (tv) 8.dp else 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.CheckCheck, tint = Marquee.colors.text, size = if (tv) 20.dp else 16.dp)
        MqText(TraktCopy.TITLE, role = TextRole.Caption, color = Marquee.colors.muted, maxLines = 1)
    }
}

/**
 * Why this play does not reach Trakt (unmapped, or this device not signed in), as a visible sentence on
 * a glass pill. It stays while the controls hide, so a viewer who never opens them still learns it.
 */
@Composable
fun TraktWarning(
    trakt: TraktPlayback?,
    modifier: Modifier = Modifier,
) {
    if (trakt != TraktPlayback.Unmapped && trakt != TraktPlayback.ThisDeviceOff) return
    val words = TraktCopy.player(trakt) ?: return
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.pill)
    Row(
        modifier
            .widthIn(max = if (tv) 720.dp else 420.dp)
            .clip(shape)
            .background(Marquee.colors.glass)
            .padding(horizontal = if (tv) 16.dp else 12.dp, vertical = if (tv) 10.dp else 8.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 10.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(
            if (trakt == TraktPlayback.Unmapped) Icons.CircleQuestionMark else Icons.Unlink,
            tint = Marquee.colors.warn,
            size = if (tv) 20.dp else 16.dp,
        )
        MqText(words, role = TextRole.Caption, color = Marquee.colors.text)
    }
}

/**
 * True while a screen reader explores by touch (TalkBack). A player's controls then stay up: swiping
 * from one to the next moves no timer, and controls that fade mid-way take the focused one with them.
 */
@Composable
fun rememberTouchExploration(): Boolean {
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
