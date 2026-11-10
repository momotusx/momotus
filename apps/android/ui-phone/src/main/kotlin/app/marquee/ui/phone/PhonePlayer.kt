package app.marquee.ui.phone

import android.media.AudioManager
import android.view.HapticFeedbackConstants
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.SubtitleBackground
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PauseFilled
import app.marquee.design.PlayFilled
import app.marquee.design.RoundButton
import app.marquee.design.TextRole
import app.marquee.player.ActionButton
import app.marquee.player.AudioStrip
import app.marquee.player.EndCard
import app.marquee.player.LoadingLayer
import app.marquee.player.PhoneSubtitleSheet
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import app.marquee.player.PlayerStatus
import app.marquee.player.PlayerVideo
import app.marquee.player.QuickShift
import app.marquee.player.ScreenBrightness
import app.marquee.player.Scrim
import app.marquee.player.Scrubber
import app.marquee.player.SkipButton
import app.marquee.player.Strip
import app.marquee.player.SubtitleStrip
import app.marquee.player.UpNextCard
import app.marquee.player.activeSegment
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.footScrim
import app.marquee.player.loadingLines
import app.marquee.player.nextName
import app.marquee.player.nextSpeed
import app.marquee.player.phoneBarFit
import app.marquee.player.rememberBarWidths
import app.marquee.player.rememberTitleMark
import app.marquee.player.rememberTouchExploration
import app.marquee.player.scrubTo
import app.marquee.player.secondLine
import app.marquee.player.speedText
import app.marquee.player.stripLift
import app.marquee.player.subtitlesDetail
import app.marquee.player.subtitlesStillListing
import app.marquee.player.upNextDue
import kotlinx.coroutines.flow.collectLatest
import kotlin.math.ceil

private const val HIDE_MS = 4_000L
private const val SEEK_MS = 10_000L
private const val HINT_MS = 700L

/** The picture's action for TalkBack, which has no tap on it. */
internal const val SHOW_CONTROLS = "Show controls"

/**
 * The player on a phone, in landscape: tap shows or hides the controls, a double tap on the left or
 * right half steps back or forward 10 s, a vertical swipe on the left half sets the brightness and on
 * the right half the volume (plans/player-gestures.md), and the position bar can be dragged. From the top: back,
 * title and the small window; the transport in the middle; at the foot one slim bar (plans/player-bar.md): Audio,
 * Speed and Versions, the position bar, then Subtitles with its quick shift, Episodes and Next, in two rows when the
 * width leaves the position bar too short (D17). Subtitles and Audio open their strips over the bar. Scrims run edge
 * to edge. In the picture-in-picture window only the picture shows.
 *
 * @param inPip the window is the small picture-in-picture one.
 * @param onPictureInPicture the viewer asked for the small window.
 * @param brightness the window's brightness as the left-hand swipe sets it, shared by every player.
 */
@Composable
fun PhonePlayer(
    controller: PlaybackController,
    autoplayNext: Boolean,
    inPip: Boolean,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onNext: (NextEpisode, String?) -> Unit,
    onPictureInPicture: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    /** The failure card's "Play {label}": open that version from the same Versions view. */
    onOffer: (String) -> Unit = {},
    /** The subtitle picker's Size: the profile's subtitle size, for this video and the next. */
    onSubtitleSize: (SubtitleSize) -> Unit = {},
    /** The picker's second line: the profile's layout, Only when paused, and the Spoken language kept with no track here. */
    onSubtitleLayout: (SubtitleLayout) -> Unit = {},
    onSecondOnPause: (Boolean) -> Unit = {},
    onSpokenKept: () -> Unit = {},
    /** The failure card's Settings: the addons (noSource). */
    onSettings: ((EmptyKind) -> Unit)? = null,
    brightness: ScreenBrightness = remember { ScreenBrightness() },
    /** The strip's Style: the profile's subtitle position and background, as Size. */
    onSubtitlePosition: (Int) -> Unit = {},
    onSubtitleBackground: (SubtitleBackground) -> Unit = {},
) {
    val stage = controller.stage
    // Back from Settings opened on the failure card: the start runs again when the player resumes, so what was added
    // plays without a press. The navigation resumes the entry on top, whether it stayed composed or not.
    LifecycleResumeEffect(controller) {
        if (controller.askAgain) {
            controller.askAgain = false
            if (controller.reopenable) controller.reopen()
        }
        onPauseOrDispose {}
    }
    val density = LocalDensity.current
    val shortWindow =
        with(density) {
            LocalWindowInfo.current.containerSize.height
                .toDp()
        } < UP_NEXT_ROOM
    // Measured from the controls; 72 dp until they have shown once.
    var controlsHeight by remember { mutableStateOf(72.dp) }
    // The open strip's height, with its Style row, as last laid out.
    var stripHeight by remember { mutableStateOf(0.dp) }
    // The foot of the title bar's controls (Back, the title, Picture in picture): the subtitles keep under it.
    var titleFoot by remember { mutableStateOf(0.dp) }
    // The foot of the transport in the middle of the screen: the subtitles keep under it while it shows.
    var transportFoot by remember { mutableStateOf(0.dp) }
    // Its top: with a strip open, lines with no room under the transport go over it.
    var transportTop by remember { mutableStateOf(0.dp) }
    // Skip or Up Next in the bottom corner, as last laid out: the subtitles' lines keep clear of it.
    var corner by remember { mutableStateOf<Rect?>(null) }
    val live = stage == Stage.Playing || stage == Stage.Paused || stage == Stage.Buffering || stage == Stage.Ended
    // Before the source shows a picture: the loading screen alone, with Back (plans/player-polish.md 1.2).
    val opening = controller.starting
    var osd by remember { mutableStateOf(true) }
    var wake by remember { mutableIntStateOf(0) }
    // The full subtitle picker (from More languages), and the strip open over the bar with its Style row.
    var picker by remember { mutableStateOf(false) }
    var strip by remember { mutableStateOf<Strip?>(null) }
    var style by remember { mutableStateOf(false) }
    val closeStrip = {
        strip = null
        style = false
    }
    // The title's logo for the loading screen, loaded once for the play.
    val mark = rememberTitleMark(controller.logo)
    // The language whose options the subtitle sheet shows; null for its languages.
    var opened by remember { mutableStateOf<String?>(null) }
    // The side of the last double tap (1 right, -1 left) and how many there were, so the same side twice restarts the hint.
    var hint by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var taps by remember { mutableIntStateOf(0) }
    val ended = stage == Stage.Ended
    val showControls = osd && live && !ended && !inPip && !opening
    val next = controller.next
    // Cancel on Up Next stops the next episode from starting by itself, for this playback: the end card shows instead.
    var upNextCancelled by remember(controller) { mutableStateOf(false) }
    val autoplay = autoplayNext && !upNextCancelled
    val showEnd = ended && !(autoplay && next != null)
    val showNext by remember(controller) {
        derivedStateOf {
            !upNextCancelled &&
                upNextDue(
                    controller.next != null,
                    controller.nextAtMs,
                    controller.positionMs,
                    controller.durationMs,
                    controller.skippedAtMs,
                )
        }
    }
    // The skippable part under the playhead (an intro, a recap), unless Up Next has taken the corner.
    val segment by remember(controller) {
        derivedStateOf { activeSegment(controller.segments, controller.positionMs) }
    }
    val showSkip =
        segment != null && !showNext && !picker && !ended && !inPip && live && controller.status == null
    // A short window has no room for the card above the controls (and the picker needs the
    // space): it waits until they hide, as they do on their timer while the episode plays.
    val showNextCard = showNext && !ended && next != null && !picker && (!showControls || !shortWindow)
    // Skip or Up Next carry a time-bound action: an open strip gives them its place (D23). Up Next counts once it is
    // due, as a short window shows its card only after the controls hide, which they never do over a strip.
    val timeBound = showSkip || (showNext && !ended && next != null && !picker)
    LaunchedEffect(timeBound) { if (timeBound) closeStrip() }

    // Seconds to the end, once a second (the card's countdown), without recomposing on every tick.
    val secondsLeft by remember(controller) {
        derivedStateOf { ceil((controller.durationMs - controller.positionMs) / 1000.0).toInt().coerceAtLeast(0) }
    }

    // The controls hide four seconds after the last touch while playing, never under TalkBack; `wake` is
    // read here and not in composition, so a touch recomposes nothing.
    val exploring = rememberTouchExploration()
    // An open strip keeps them up; each shift tick wakes them, so a held shift never loses them.
    LaunchedEffect(osd, stage, picker, strip, showNext, exploring) {
        if (osd && stage == Stage.Playing && !picker && strip == null && !exploring) {
            snapshotFlow { wake }.collectLatest {
                kotlinx.coroutines.delay(HIDE_MS)
                osd = false
            }
        }
    }
    LaunchedEffect(hint) {
        if (hint != null) {
            kotlinx.coroutines.delay(HINT_MS)
            hint = null
        }
    }
    LaunchedEffect(stage) {
        if (stage == Stage.Ended && autoplay && next != null) onNext(next, controller.bingeGroup)
    }

    // The level swipes: the level is read only by the indicator, so a swipe's moves recompose nothing here.
    val window = LocalActivity.current?.window
    val context = LocalContext.current
    val view = LocalView.current
    val volume = remember(context) { context.getSystemService(AudioManager::class.java)?.let(::MediaVolume) }
    val swipeLevel = remember { mutableFloatStateOf(0f) }
    var levelSide by remember { mutableStateOf(SwipeSide.Volume) }
    var levelShown by remember { mutableStateOf(false) }
    var swiping by remember { mutableStateOf(false) }
    LaunchedEffect(swiping, levelShown) {
        if (!swiping && levelShown) {
            kotlinx.coroutines.delay(LEVEL_HOLD_MS)
            levelShown = false
        }
    }
    // The system's own edges, from the window as it is now (rotation, the back gesture's width), never below the floors.
    val gestureInsets = WindowInsets.systemGestures
    val direction = LocalLayoutDirection.current
    val bands by rememberUpdatedState(
        with(density) {
            EdgeBands(
                left = maxOf(gestureInsets.getLeft(density, direction).toFloat(), SIDE_BAND.toPx()),
                top = maxOf(gestureInsets.getTop(density).toFloat(), END_BAND.toPx()),
                right = maxOf(gestureInsets.getRight(density, direction).toFloat(), SIDE_BAND.toPx()),
                bottom = maxOf(gestureInsets.getBottom(density).toFloat(), END_BAND.toPx()),
            )
        },
    )
    // A list or strip that was open when the film ended closes: the end card is the only thing to answer.
    LaunchedEffect(ended) {
        if (ended) {
            picker = false
            closeStrip()
        }
    }
    // Back: the picker's language, the picker, the Style row, the strip, then the player (section 6).
    BackHandler {
        when {
            picker && opened != null -> opened = null
            picker -> picker = false
            style -> style = false
            strip != null -> strip = null
            else -> onBack()
        }
    }

    Box(
        modifier
            .fillMaxSize()
            .background(Color.Black)
            // TalkBack has no tap on the picture: a double tap on it shows the controls.
            .semantics {
                if (live && !opening) {
                    onClick(label = SHOW_CONTROLS) {
                        osd = true
                        wake++
                        true
                    }
                }
            }.pointerInput(live && !opening) {
                // While it loads there is nothing to show or seek: a tap never hides Back.
                if (!live || opening) return@pointerInput
                detectTapGestures(
                    onTap = {
                        // A tap on the picture while a strip or the picker is open closes it, nothing else (D21).
                        when {
                            strip != null -> closeStrip()
                            picker -> picker = false
                            else -> osd = !osd
                        }
                        wake++
                    },
                    onDoubleTap = { at ->
                        // With a strip open the first taps only close it: seeking starts from the next tap (D21).
                        if (strip != null) {
                            closeStrip()
                            wake++
                            return@detectTapGestures
                        }
                        val forward = at.x > size.width / 2
                        controller.seekBy(if (forward) SEEK_MS else -SEEK_MS)
                        hint = (if (forward) 1 else -1) to ++taps
                    },
                )
            },
    ) {
        // While the controls show, the subtitles sit between them (and an open strip) and the title bar, at the foot
        // as ever.
        val stripCover = if (strip != null) stripHeight + stripLift() else 0.dp
        PlayerVideo(
            controller.player,
            Modifier.fillMaxSize(),
            controller.subtitleSize,
            subtitleLift = if (showControls) controlsHeight + stripCover else 0.dp,
            subtitleCeiling = if (showControls) maxOf(titleFoot, transportFoot) else 0.dp,
            subtitleOver = if (showControls && strip != null) transportTop else 0.dp,
            subtitleCorner = corner.takeIf { showSkip || showNextCard },
            addonCues = { controller.addonCues() }.takeIf { controller.addonOn },
            videoRatio = controller.videoRatio,
            secondCues = { controller.secondCues() }.takeIf { controller.secondOn },
            subtitleLayout = controller.subtitleLayout,
            secondOnPause = controller.secondOnPause,
            subtitleTop = if (showControls) titleFoot else 0.dp,
            subtitlePosition = controller.subtitlePosition,
            subtitleBackground = controller.subtitleBackground,
        )
        // The level swipes' own layer, right over the picture: the controls, lists and cards drawn after it are hit
        // first, so a touch on any of them never reaches it, and the picture's taps (on the Box) still see the rest.
        Box(
            Modifier.fillMaxSize().levelSwipes(
                // Never over a list, the loading or end screens, the small window or TalkBack (its two-finger
                // scroll arrives as a one-finger drag).
                enabled = live && !opening && !ended && !picker && strip == null && !inPip && !exploring,
                bands = { bands },
                onStart = { side ->
                    when (side) {
                        SwipeSide.Brightness -> window?.let { brightness.start(context.contentResolver) }
                        SwipeSide.Volume -> volume?.start()
                    }?.also {
                        swipeLevel.floatValue = it
                        levelSide = side
                        levelShown = true
                        swiping = true
                        wake++
                    }
                },
                onLevel = { side, to ->
                    val took =
                        when (side) {
                            SwipeSide.Brightness -> to.also { window?.let { brightness.set(it, to) } }
                            SwipeSide.Volume -> volume?.set(to) ?: to
                        }
                    // Compose's SegmentTick is the API 34 constant itself, nothing below it: CLOCK_TICK runs from 24 on.
                    val end = reachedEnd(swipeLevel.floatValue, took)
                    if (end) view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                    swipeLevel.floatValue = took
                    // The controls stay up while the finger moves.
                    wake++
                },
                onEnd = { swiping = false },
            ),
        )

        // The loading screen until the picture comes, and over the paused picture while it stalls with the controls
        // hidden (with them up the Play circle turns instead).
        val stalled = stage == Stage.Buffering && controller.picture && !showControls
        if ((opening || stalled) && !inPip) {
            LoadingLayer(
                title = controller.title,
                mark = mark,
                backdrop = controller.backdrop,
                fill = { controller.loadingFill },
                lines = loadingLines(controller.status, controller.start?.caption),
                opening = opening,
                // Choose another version, 20 s after the loading screen showed (plans/versions-android-spec.md, 6).
                choose = onVersions.takeIf { opening || stalled },
                // When the core offers it: 20 s into the start, 30 s into a stall (one clock, the core's).
                chooseShown = controller.chooseAnother,
            )
        }

        // Scrims: edge to edge, 120 dp at the top (60% black) and 200 dp at the foot (72%).
        AnimatedVisibility(
            visible = ((osd && !ended) || !live || opening) && !inPip,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box(Modifier.fillMaxSize()) {
                // While loading, Back alone: the mark under it names the title.
                if (!opening) Scrim(top = true, height = 120.dp, peak = 0.85f)
                Row(
                    Modifier
                        .windowInsetsPadding(WindowInsets.displayCutout)
                        .padding(16.dp)
                        .onGloballyPositioned { titleFoot = with(density) { it.boundsInRoot().bottom.toDp() } },
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RoundButton(Icons.ArrowLeft, "Back", onBack)
                    Column(Modifier.weight(1f)) {
                        if (!opening) {
                            MqText(controller.title, role = TextRole.Title3, maxLines = 1)
                            controller.episode?.let {
                                MqText(it, role = TextRole.Callout, maxLines = 1)
                            }
                        }
                    }
                    if (live) RoundButton(Icons.PictureInPicture2, "Picture in picture", onPictureInPicture)
                }
            }
        }
        // Above the top scrim: the status sits under the title bar on a phone.
        if (!inPip) {
            hint?.let { (side, _) ->
                SeekHint(
                    forward = side > 0,
                    modifier =
                        Modifier
                            .align(if (side > 0) Alignment.CenterEnd else Alignment.CenterStart)
                            .padding(horizontal = 64.dp),
                )
            }
            AnimatedVisibility(
                visible = levelShown,
                modifier =
                    Modifier
                        .align(if (levelSide == SwipeSide.Brightness) Alignment.CenterStart else Alignment.CenterEnd)
                        .windowInsetsPadding(WindowInsets.displayCutout)
                        .padding(horizontal = 40.dp),
                enter = fadeIn(tween(Marquee.focusMs)),
                exit = fadeOut(tween(Marquee.panelMs)),
            ) {
                LevelIndicator(levelSide, { swipeLevel.floatValue })
            }
            // While the loading screen shows, it says the status under the mark.
            val status = controller.status.takeUnless { opening || stalled }
            if (stage == Stage.Failed || status != null || controller.notice != null) {
                PlayerStatus(
                    stage = stage,
                    status = status,
                    notice = controller.notice,
                    problem = controller.problem,
                    attempts = controller.attempts,
                    onVersions = onVersions,
                    offer = controller.offer,
                    onOffer = onOffer,
                    onRetry =
                        when {
                            controller.retryable -> controller::retry
                            controller.reopenable -> controller::reopen
                            else -> null
                        },
                    tryingAgain = controller.tryingAgain,
                    detail = controller.openError,
                    cannotOpen = controller.openError != null,
                    empty = controller.empty,
                    kept = controller.keptKey,
                    onSettings =
                        onSettings?.let { go ->
                            { kind: EmptyKind ->
                                controller.askAgain = true
                                go(kind)
                            }
                        },
                    onBack = onBack,
                    focus = remember { FocusRequester() },
                    modifier =
                        Modifier
                            .align(Alignment.TopStart)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(start = 24.dp, end = 24.dp, top = 88.dp),
                )
            }
        }

        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            // The scrim can be taller than the controls: they keep to the foot. Its top follows an open strip, so
            // the strip's chips sit on its dark part (section 5, scrim).
            Box(contentAlignment = Alignment.BottomCenter) {
                Scrim(
                    top = false,
                    height = footScrim(controlsHeight + stripCover, SCRIM_LEAST),
                    peak = 0.85f,
                    modifier = Modifier.align(Alignment.BottomCenter),
                )
                Controls(
                    controller = controller,
                    strip = strip,
                    onStrip = { opening ->
                        style = false
                        strip = opening.takeUnless { it == strip }
                        wake++
                    },
                    onVersions = onVersions,
                    onEpisodes = onEpisodes,
                    onNext = next?.let { { onNext(it, controller.bingeGroup) } },
                    onTouch = { wake++ },
                    // The last height they had: the strips, the skip button and Up Next sit just above it.
                    modifier =
                        Modifier.onSizeChanged {
                            if (it.height > 0) controlsHeight = with(density) { it.height.toDp() }
                        },
                )
            }
        }
        // The strips over the bar's ends (section 5): touches on them stay on them.
        if (showControls) {
            val cardLift =
                corner?.takeIf { showSkip || showNextCard }?.let { with(density) { it.height.toDp() } + stripLift() }
                    ?: 0.dp
            val placed =
                Modifier
                    .windowInsetsPadding(WindowInsets.displayCutout)
                    // Over Skip or Up Next when one shows in the corner: a strip opened then never meets it (D23).
                    .padding(start = 16.dp, end = 16.dp, bottom = controlsHeight + stripLift() + cardLift)
                    .onSizeChanged { stripHeight = with(density) { it.height.toDp() } }
                    .pointerInput(Unit) { detectTapGestures { wake++ } }
            when (strip) {
                Strip.Subtitles -> {
                    SubtitleStrip(
                        controller,
                        style = style,
                        onStyle = {
                            style = it
                            wake++
                        },
                        onMore = {
                            closeStrip()
                            opened = null
                            picker = true
                        },
                        onClose = closeStrip,
                        onSize = onSubtitleSize,
                        onPosition = onSubtitlePosition,
                        onBackground = onSubtitleBackground,
                        modifier = Modifier.align(Alignment.BottomEnd).then(placed),
                    )
                }

                Strip.Audio -> {
                    AudioStrip(
                        controller,
                        onVersions = onVersions,
                        onClose = closeStrip,
                        modifier = Modifier.align(Alignment.BottomStart).then(placed),
                    )
                }

                null -> {
                    Unit
                }
            }
        }
        // The transport in the middle of the screen, as on the iPhone and the web; the bar and the options
        // keep to the foot.
        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.Center),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Transport(
                controller,
                controller.offersPause,
                waiting = stage == Stage.Buffering,
                onTouch = { wake++ },
                modifier =
                    Modifier.onGloballyPositioned {
                        val box = it.boundsInRoot()
                        transportTop = with(density) { box.top.toDp() }
                        transportFoot = with(density) { box.bottom.toDp() }
                    },
                closeStrip = closeStrip.takeIf { strip != null },
            )
        }

        if (!inPip) {
            // The full picker, from More languages: a side sheet on the right, so the picture stays in view.
            if (picker) {
                // The player dims under the sheet; a tap there closes it.
                Box(
                    Modifier
                        .fillMaxSize()
                        .background(Color.Black.copy(alpha = 0.45f))
                        .pointerInput(Unit) { detectTapGestures { picker = false } },
                )
                PhoneSubtitleSheet(
                    rows = controller.text,
                    opened = opened,
                    onOpen = { opened = it },
                    onPick = controller::selectText,
                    onDone = { picker = false },
                    modifier =
                        Modifier
                            .align(Alignment.CenterEnd)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(top = 12.dp, bottom = 12.dp, end = 16.dp)
                            .widthIn(max = 420.dp)
                            .fillMaxWidth()
                            .fillMaxHeight()
                            .pointerInput(Unit) { detectTapGestures { } },
                    note = controller.subtitlePanelNote,
                    timing = controller.quickShift,
                    onDelay = controller::setSubtitleDelay,
                    textSize = controller.subtitleSize,
                    onSize = onSubtitleSize,
                    second = controller.secondLine(onSubtitleLayout, onSecondOnPause, onSpokenKept),
                    secondTab = controller.secondTab,
                    onTab = { controller.secondTab = it },
                )
            }
            if (showSkip) {
                segment?.let { skip ->
                    SkipButton(
                        segment = skip,
                        onSkip = {
                            controller.skip(skip)
                            wake++
                        },
                        modifier =
                            Modifier
                                .align(Alignment.BottomEnd)
                                .windowInsetsPadding(WindowInsets.displayCutout)
                                .padding(end = 16.dp, bottom = if (showControls) controlsHeight + 8.dp else 24.dp)
                                .onGloballyPositioned { corner = it.boundsInRoot() },
                    )
                }
            }
            if (showNextCard && next != null) {
                UpNextCard(
                    next = next,
                    countdown = secondsLeft.takeIf { autoplay },
                    onPlay = { onNext(next, controller.bingeGroup) },
                    onCancel = { upNextCancelled = true },
                    modifier =
                        Modifier
                            .align(Alignment.BottomEnd)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(end = 16.dp, bottom = if (showControls) controlsHeight + 8.dp else 24.dp)
                            .onGloballyPositioned { corner = it.boundsInRoot() },
                )
            }
            if (showEnd) {
                EndCard(
                    title = controller.title,
                    next = next,
                    onNext = { next?.let { onNext(it, controller.bingeGroup) } },
                    onAgain = {
                        upNextCancelled = false
                        controller.play()
                    },
                    onBack = onBack,
                    focus = remember { FocusRequester() },
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}

/** The double-tap answer: a 96 dp circle on the tapped side, 40% black, two chevrons and "10 seconds". */
@Composable
private fun SeekHint(
    forward: Boolean,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    Column(
        modifier.size(96.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.4f)),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Row {
            val icon = if (forward) Icons.ChevronRight else Icons.ChevronLeft
            MqIcon(icon, tint = colors.text, size = 22.dp)
            MqIcon(icon, tint = colors.text, size = 22.dp)
        }
        MqText("10 seconds", role = TextRole.Caption, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

/**
 * The bar (plans/player-bar.md, phone form): Audio, Speed and Versions at the left, the position bar between its
 * clocks, then Subtitles, the quick shift while a subtitle can be timed, Episodes and Next. Pills are icons alone
 * (their names and values spoken); [phoneBarFit] decides, from the width and film or series alone, whether the shift
 * says Earlier and Later in words and whether the bar takes a second row.
 */
@Composable
private fun Controls(
    controller: PlaybackController,
    strip: Strip?,
    onStrip: (Strip) -> Unit,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onNext: (() -> Unit)?,
    onTouch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val widths = rememberBarWidths()
    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.displayCutout)
            .padding(horizontal = 16.dp)
            .padding(bottom = 12.dp)
            .testTag(BAR_TAG)
            // A tap between the pills is a tap on the bar: it never reaches the picture (D21).
            .pointerInput(Unit) { detectTapGestures { onTouch() } },
    ) {
        val series = onEpisodes != null
        // Left: Audio, Speed, Versions; right: Subtitles, then Episodes and Next for a series, the shift counted apart.
        val right = if (series) 3 else 1
        val groups = widths.pill * (3 + right) + PILL_GAP * (2 + right) + ROW_GAP * 2
        val clock = widths.text("-0:00:00")
        val fit = phoneBarFit(maxWidth, groups, clock * 2 + ROW_GAP * 2, widths.shiftWords, widths.shiftChevrons)
        val left: @Composable () -> Unit = { LeftPills(controller, strip, onStrip, onVersions, onTouch) }
        val pills: @Composable () -> Unit = {
            RightPills(controller, strip, onStrip, onEpisodes, onNext, fit.shiftWords, onTouch)
        }
        if (fit.rows == 1) {
            Row(horizontalArrangement = Arrangement.spacedBy(ROW_GAP), verticalAlignment = Alignment.CenterVertically) {
                left()
                Track(controller, onTouch, Modifier.weight(1f))
                pills()
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) { Track(controller, onTouch, Modifier.weight(1f)) }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    left()
                    Spacer(Modifier.weight(1f))
                    pills()
                }
            }
        }
    }
}

@Composable
private fun LeftPills(
    controller: PlaybackController,
    strip: Strip?,
    onStrip: (Strip) -> Unit,
    onVersions: () -> Unit,
    onTouch: () -> Unit,
) {
    val names = StreamChoiceCopy.Bar.Names
    Row(horizontalArrangement = Arrangement.spacedBy(PILL_GAP), verticalAlignment = Alignment.CenterVertically) {
        // Audio once the file's tracks are known, as before.
        if (controller.audio.isNotEmpty()) {
            ActionButton(
                Icons.AudioLines,
                names.AUDIO,
                controller.audioDetail(),
                { onStrip(Strip.Audio) },
                words = false,
                expanded = strip == Strip.Audio,
            )
        }
        ActionButton(Icons.Gauge, names.SPEED, speedText(controller.speed), {
            onTouch()
            controller.changeSpeed(nextSpeed(controller.speed))
        }, words = false)
        ActionButton(Icons.Layers, names.VERSIONS, controller.versionLabel, onVersions, words = false)
    }
}

@Composable
private fun RightPills(
    controller: PlaybackController,
    strip: Strip?,
    onStrip: (Strip) -> Unit,
    onEpisodes: (() -> Unit)?,
    onNext: (() -> Unit)?,
    shiftWords: Boolean,
    onTouch: () -> Unit,
) {
    val names = StreamChoiceCopy.Bar.Names
    Row(horizontalArrangement = Arrangement.spacedBy(PILL_GAP), verticalAlignment = Alignment.CenterVertically) {
        ActionButton(
            Icons.Captions,
            names.SUBTITLES,
            controller.subtitlesDetail(),
            { onStrip(Strip.Subtitles) },
            busy = controller.subtitlesStillListing(),
            words = false,
            expanded = strip == Strip.Subtitles,
        )
        // Only while the subtitle on can be timed (D4); each tick keeps the controls up.
        controller.quickShift?.let { delay ->
            QuickShift(delay, {
                onTouch()
                controller.setSubtitleDelay(it)
            }, words = shiftWords)
        }
        onEpisodes?.let { ActionButton(Icons.ListVideo, names.EPISODES, null, it, words = false) }
        val next = controller.next
        if (next != null && onNext != null) {
            ActionButton(
                Icons.SkipForward,
                StreamChoiceCopy.Bar.NEXT,
                null,
                onNext,
                words = false,
                spoken = nextName(next),
            )
        }
    }
}

/** The clocks and the position bar between them, dragged or tapped to seek. */
@Composable
private fun Track(
    controller: PlaybackController,
    onTouch: () -> Unit,
    modifier: Modifier,
) {
    var dragging by remember { mutableStateOf<Long?>(null) }
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(ROW_GAP),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(clockText(dragging ?: controller.positionMs), role = TextRole.Caption, maxLines = 1)
        Scrubber(
            position = { controller.positionMs },
            duration = { controller.durationMs },
            buffered = { controller.bufferedMs },
            preview = { dragging },
            emphasized = dragging != null,
            onSeek = controller::seekTo,
            modifier =
                Modifier
                    .weight(1f)
                    .height(48.dp)
                    .pointerInput(Unit) {
                        detectTapGestures { at ->
                            onTouch()
                            // No length yet (the first moments, or a source being set again): nothing to seek in.
                            scrubTo(at.x, size.width, controller.durationMs)?.let(controller::seekTo)
                        }
                    }.pointerInput(Unit) {
                        detectHorizontalDragGestures(
                            onDragStart = { at -> dragging = scrubTo(at.x, size.width, controller.durationMs) },
                            onHorizontalDrag = { change, _ ->
                                onTouch()
                                dragging = scrubTo(change.position.x, size.width, controller.durationMs) ?: dragging
                            },
                            onDragEnd = {
                                dragging?.let(controller::seekTo)
                                dragging = null
                            },
                            onDragCancel = { dragging = null },
                        )
                    },
        )
        MqText(
            "-" + clockText((controller.durationMs - controller.positionMs).coerceAtLeast(0)),
            role = TextRole.Caption,
            maxLines = 1,
        )
    }
}

/**
 * Back 10 seconds, play or pause, forward 10 seconds; while [waiting], the spinner turns in the Play circle. While a
 * strip is open ([closeStrip] given) a press only closes it: no pause and no seek (D21).
 */
@Composable
private fun Transport(
    controller: PlaybackController,
    playing: Boolean,
    waiting: Boolean,
    onTouch: () -> Unit,
    modifier: Modifier = Modifier,
    closeStrip: (() -> Unit)? = null,
) {
    val press = { action: () -> Unit ->
        onTouch()
        if (closeStrip != null) closeStrip() else action()
    }
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(18.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RoundButton(Icons.RotateCcw, "Back 10 seconds", { press { controller.seekBy(-SEEK_MS) } }, badge = "10")
        RoundButton(
            icon = if (playing) PauseFilled else PlayFilled,
            label = if (playing) "Pause" else "Play",
            onClick = { press(controller::togglePlay) },
            main = true,
            busy = waiting,
        )
        RoundButton(Icons.RotateCw, "Forward 10 seconds", { press { controller.seekBy(SEEK_MS) } }, badge = "10")
    }
}

/** Between the pills of a group (the pick's 6 px on a phone frame). */
private val PILL_GAP = 6.dp

/** Between the groups, the clocks and the position bar of the row. */
private val ROW_GAP = 8.dp

/** The least height of the foot scrim (the pick's phone frame: 120 px over a bar it now outgrows). */
private val SCRIM_LEAST = 160.dp

/** The bar, for tests of the layout. */
internal const val BAR_TAG = "player-bar"

/** How long the level indicator stays after a swipe ends, before it fades. */
private const val LEVEL_HOLD_MS = 800L

/** The bands along the sides and the ends where a touch is the system's, at the least. */
private val SIDE_BAND = 24.dp
private val END_BAND = 32.dp

/** Below this window height Up Next and the controls do not show together. */
private val UP_NEXT_ROOM = 420.dp
