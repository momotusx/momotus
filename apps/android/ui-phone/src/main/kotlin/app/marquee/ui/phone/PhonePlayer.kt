package app.marquee.ui.phone

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.marquee.core.model.NextEpisode
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PauseFilled
import app.marquee.design.PlayFilled
import app.marquee.design.RoundButton
import app.marquee.design.Spinner
import app.marquee.design.TextRole
import app.marquee.design.endFade
import app.marquee.player.ActionButton
import app.marquee.player.EndCard
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import app.marquee.player.PlayerStatus
import app.marquee.player.PlayerVideo
import app.marquee.player.Scrim
import app.marquee.player.Scrubber
import app.marquee.player.SkipButton
import app.marquee.player.TrackPanel
import app.marquee.player.TraktMark
import app.marquee.player.TraktWarning
import app.marquee.player.UpNextCard
import app.marquee.player.activeSegment
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.nextSpeed
import app.marquee.player.rememberTouchExploration
import app.marquee.player.scrubTo
import app.marquee.player.speedText
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

private enum class Panel { Audio, Subtitles }

/**
 * The player on a phone, in landscape: tap shows or hides the controls, a double tap on the left or
 * right half steps back or forward 10 s, and the position bar can be dragged. From the top: back,
 * title and the small window; from the foot: the transport, the position bar, then the actions,
 * with the transport never crossing the bar. Scrims run edge to edge. In the picture-in-picture
 * window only the picture shows.
 *
 * @param inPip the window is the small picture-in-picture one.
 * @param onPictureInPicture the viewer asked for the small window.
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
) {
    val stage = controller.stage
    val density = LocalDensity.current
    val shortWindow =
        with(density) {
            LocalWindowInfo.current.containerSize.height
                .toDp()
        } < UP_NEXT_ROOM
    // Measured from the controls; 200 dp until they have shown once.
    var controlsHeight by remember { mutableStateOf(200.dp) }
    // The foot of the title bar's controls (Back, the title, Picture in picture): the subtitles keep under it.
    var titleFoot by remember { mutableStateOf(0.dp) }
    // The foot of the transport in the middle of the screen: the subtitles keep under it while it shows.
    var transportFoot by remember { mutableStateOf(0.dp) }
    // Skip or Up Next in the bottom corner, as last laid out: the subtitles' lines keep clear of it.
    var corner by remember { mutableStateOf<Rect?>(null) }
    val live = stage == Stage.Playing || stage == Stage.Paused || stage == Stage.Buffering || stage == Stage.Ended
    var osd by remember { mutableStateOf(true) }
    var wake by remember { mutableIntStateOf(0) }
    var panel by remember { mutableStateOf<Panel?>(null) }
    // The side of the last double tap (1 right, -1 left) and how many there were, so the same side twice restarts the hint.
    var hint by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var taps by remember { mutableIntStateOf(0) }
    val ended = stage == Stage.Ended
    val showControls = osd && live && !ended && !inPip
    val next = controller.next
    val showEnd = ended && !(autoplayNext && next != null)
    val showNext by remember(controller) {
        derivedStateOf {
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
        segment != null && !showNext && panel == null && !ended && !inPip && live && controller.status == null
    // A short window has no room for the card above the controls (and a track panel needs the
    // space): it waits until they hide, as they do on their timer while the episode plays.
    val showNextCard = showNext && !ended && next != null && panel == null && (!showControls || !shortWindow)

    // Seconds to the end, once a second (the card's countdown), without recomposing on every tick.
    val secondsLeft by remember(controller) {
        derivedStateOf { ceil((controller.durationMs - controller.positionMs) / 1000.0).toInt().coerceAtLeast(0) }
    }

    // The controls hide four seconds after the last touch while playing, never under TalkBack; `wake` is
    // read here and not in composition, so a touch recomposes nothing.
    val exploring = rememberTouchExploration()
    LaunchedEffect(osd, stage, panel, showNext, exploring) {
        if (osd && stage == Stage.Playing && panel == null && !exploring) {
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
        if (stage == Stage.Ended && autoplayNext && next != null) onNext(next, controller.bingeGroup)
    }
    // A list that was open when the film ended closes: the end card is the only thing to answer.
    LaunchedEffect(ended) { if (ended) panel = null }
    BackHandler { if (panel != null) panel = null else onBack() }

    Box(
        modifier
            .fillMaxSize()
            .background(Color.Black)
            // TalkBack has no tap on the picture: a double tap on it shows the controls.
            .semantics {
                if (live) {
                    onClick(label = SHOW_CONTROLS) {
                        osd = true
                        wake++
                        true
                    }
                }
            }.pointerInput(live) {
                if (!live) return@pointerInput
                detectTapGestures(
                    onTap = {
                        // A tap on the picture while a list is open closes the list, nothing else.
                        if (panel != null) {
                            panel = null
                        } else {
                            osd = !osd
                        }
                        wake++
                    },
                    onDoubleTap = { at ->
                        val forward = at.x > size.width / 2
                        controller.seekBy(if (forward) SEEK_MS else -SEEK_MS)
                        hint = (if (forward) 1 else -1) to ++taps
                    },
                )
            },
    ) {
        // While the controls show, the subtitles sit between them and the title bar, at the foot as ever.
        PlayerVideo(
            controller.player,
            Modifier.fillMaxSize(),
            controller.subtitleSize,
            subtitleLift = if (showControls) controlsHeight else 0.dp,
            subtitleCeiling = if (showControls) maxOf(titleFoot, transportFoot) else 0.dp,
            subtitleCorner = corner.takeIf { showSkip || showNextCard },
        )

        // Scrims: edge to edge, 120 dp at the top (60% black) and 200 dp at the foot (72%).
        AnimatedVisibility(
            visible = ((osd && !ended) || !live) && !inPip,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box(Modifier.fillMaxSize()) {
                Scrim(top = true, height = 120.dp, peak = 0.85f)
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
                        MqText(controller.title, role = TextRole.Title3, maxLines = 1)
                        controller.episode?.let {
                            MqText(it, role = TextRole.Callout, maxLines = 1)
                        }
                        TraktMark(controller.trakt, Modifier.padding(top = 4.dp))
                    }
                    if (live) RoundButton(Icons.PictureInPicture2, "Picture in picture", onPictureInPicture)
                }
            }
        }
        // Not recorded on Trakt: said under the title row, and still there when the controls hide.
        if (!inPip) {
            TraktWarning(
                controller.trakt,
                Modifier
                    .align(Alignment.TopStart)
                    .windowInsetsPadding(WindowInsets.displayCutout)
                    .padding(start = 16.dp, top = 84.dp, end = 16.dp),
            )
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
            val said = controller.status != null || controller.notice != null
            if (stage == Stage.Opening || stage == Stage.Failed || said) {
                PlayerStatus(
                    stage = stage,
                    status = controller.status,
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
                    onBack = onBack,
                    focus = remember { FocusRequester() },
                    modifier =
                        Modifier
                            .align(Alignment.TopStart)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(start = 24.dp, end = 24.dp, top = 88.dp),
                )
            }
            // A notice while the buffer fills keeps the mark in the middle.
            // With the controls up the spinner turns in the Play circle instead (the transport is in the middle).
            if (stage == Stage.Buffering && controller.status == null && !showControls) {
                Spinner(Modifier.align(Alignment.Center))
            }
        }

        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            // The scrim can be taller than the controls: they keep to the foot.
            Box(contentAlignment = Alignment.BottomCenter) {
                Scrim(
                    top = false,
                    height = 240.dp,
                    peak = 0.85f,
                    modifier = Modifier.align(Alignment.BottomCenter),
                )
                Controls(
                    controller = controller,
                    onAudio = { panel = Panel.Audio },
                    onSubtitles = { panel = Panel.Subtitles },
                    onVersions = onVersions,
                    onEpisodes = onEpisodes,
                    onTouch = { wake++ },
                    // The last height they had: the skip button and Up Next sit just above it.
                    modifier =
                        Modifier.onSizeChanged {
                            if (it.height > 0) controlsHeight = with(density) { it.height.toDp() }
                        },
                )
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
                        transportFoot = with(density) { it.boundsInRoot().bottom.toDp() }
                    },
            )
        }

        if (!inPip) {
            // The track lists are a side sheet on the right, 480 dp wide, so the picture stays in view.
            val sheet =
                Modifier
                    .align(Alignment.CenterEnd)
                    .windowInsetsPadding(WindowInsets.displayCutout)
                    .padding(16.dp)
                    .width(480.dp)
                    // Touches on the sheet stay on the sheet: they must not reach the controls under it.
                    .pointerInput(Unit) { detectTapGestures { } }
            when (panel) {
                Panel.Audio -> {
                    TrackPanel("Audio", controller.audio, onPick = {
                        controller.selectAudio(it)
                        panel = null
                    }, modifier = sheet)
                }

                Panel.Subtitles -> {
                    TrackPanel(
                        "Subtitles",
                        controller.text,
                        onPick = {
                            controller.selectText(it)
                            panel = null
                        },
                        modifier = sheet,
                        note = controller.subtitlePanelNote,
                        timing = controller.subtitleDelayMs.takeIf { controller.subtitleOn },
                        onShift = controller::shiftSubtitles,
                    )
                }

                null -> {
                    Unit
                }
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
                    countdown = secondsLeft.takeIf { autoplayNext },
                    onPlay = { onNext(next, controller.bingeGroup) },
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
                    onAgain = controller::play,
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

@Composable
private fun Controls(
    controller: PlaybackController,
    onAudio: () -> Unit,
    onSubtitles: () -> Unit,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onTouch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var dragging by remember { mutableStateOf<Long?>(null) }
    Column(
        modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.displayCutout)
            .padding(horizontal = 16.dp)
            .padding(bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            MqText(clockText(dragging ?: controller.positionMs), role = TextRole.Caption)
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
            )
        }
        Actions(controller, onAudio, onSubtitles, onVersions, onEpisodes, onTouch, Modifier.fillMaxWidth())
    }
}

/** Back 10 seconds, play or pause, forward 10 seconds; while [waiting], the spinner turns in the Play circle. */
@Composable
private fun Transport(
    controller: PlaybackController,
    playing: Boolean,
    waiting: Boolean,
    onTouch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(18.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RoundButton(
            Icons.RotateCcw,
            "Back 10 seconds",
            {
                onTouch()
                controller.seekBy(-SEEK_MS)
            },
            badge = "10",
        )
        RoundButton(
            icon = if (playing) PauseFilled else PlayFilled,
            label = if (playing) "Pause" else "Play",
            onClick = {
                onTouch()
                controller.togglePlay()
            },
            main = true,
            busy = waiting,
        )
        RoundButton(
            Icons.RotateCw,
            "Forward 10 seconds",
            {
                onTouch()
                controller.seekBy(SEEK_MS)
            },
            badge = "10",
        )
    }
}

/**
 * Audio, Subtitles, Versions, Speed, then Episodes (the design audit's order for every platform), as one
 * group in the middle under the bar, never spread edge to edge on a wide phone; past the width (a small
 * phone, a large font, long track names) the row scrolls, so no action is squeezed under its 48 dp target.
 */
@Composable
private fun Actions(
    controller: PlaybackController,
    onAudio: () -> Unit,
    onSubtitles: () -> Unit,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onTouch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scroll = rememberScrollState()
    // The group sits in the middle of its width (the scroll is as wide as the actions, up to that width);
    // a fade at the end says the row goes on when the later actions are out of sight.
    Box(modifier, contentAlignment = Alignment.Center) {
        Row(
            Modifier.endFade(scroll.canScrollForward).horizontalScroll(scroll).testTag(ACTIONS_TAG),
            horizontalArrangement = Arrangement.spacedBy(ACTION_GAP),
        ) {
            if (controller.audio.isNotEmpty()) {
                ActionButton(Icons.Volume2, "Audio", controller.audioDetail(), onAudio)
            }
            ActionButton(
                Icons.Captions,
                "Subtitles",
                controller.subtitlesDetail(),
                onSubtitles,
                busy = controller.subtitlesStillListing(),
            )
            ActionButton(Icons.Layers, "Versions", controller.versionLabel, onVersions)
            ActionButton(Icons.Gauge, "Speed", speedText(controller.speed), {
                onTouch()
                controller.changeSpeed(nextSpeed(controller.speed))
            })
            onEpisodes?.let { ActionButton(Icons.ListVideo, "Episodes", null, it) }
        }
    }
}

/** Between two actions of the options row: a group, not a spread. */
private val ACTION_GAP = 28.dp

/** The options row, for tests of the layout. */
internal const val ACTIONS_TAG = "player-actions"

/** Below this window height Up Next and the controls do not show together. */
private val UP_NEXT_ROOM = 420.dp
