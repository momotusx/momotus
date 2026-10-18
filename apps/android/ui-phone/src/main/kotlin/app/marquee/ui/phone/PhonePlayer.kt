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
import androidx.compose.foundation.layout.fillMaxHeight
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
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
import app.marquee.player.UpNextCard
import app.marquee.player.activeSegment
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.nextSpeed
import app.marquee.player.scrubTo
import app.marquee.player.speedText
import app.marquee.player.subtitlesDetail
import app.marquee.player.upNextDue
import kotlinx.coroutines.flow.collectLatest
import kotlin.math.ceil

private const val HIDE_MS = 4_000L
private const val SEEK_MS = 10_000L
private const val HINT_MS = 700L

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
) {
    val stage = controller.stage
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

    // Seconds to the end, once a second (the card's countdown), without recomposing on every tick.
    val secondsLeft by remember(controller) {
        derivedStateOf { ceil((controller.durationMs - controller.positionMs) / 1000.0).toInt().coerceAtLeast(0) }
    }

    // The controls hide four seconds after the last touch while playing; `wake` is read here and not in
    // composition, so a touch recomposes nothing.
    LaunchedEffect(osd, stage, panel, showNext) {
        if (osd && stage == Stage.Playing && panel == null && !showNext) {
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
            .pointerInput(live) {
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
        PlayerVideo(controller.player, Modifier.fillMaxSize())

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
            if (stage == Stage.Opening || stage == Stage.Failed || controller.status != null) {
                PlayerStatus(
                    stage = stage,
                    status = controller.status,
                    problem = controller.problem,
                    attempts = controller.attempts,
                    onVersions = onVersions,
                    onBack = onBack,
                    focus = remember { FocusRequester() },
                    modifier =
                        Modifier
                            .align(Alignment.TopStart)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(start = 24.dp, end = 24.dp, top = 88.dp),
                )
            } else if (stage == Stage.Buffering) {
                Spinner(Modifier.align(Alignment.Center))
            }
        }

        // Scrims: edge to edge, 120 dp at the top (60% black) and 200 dp at the foot (72%).
        AnimatedVisibility(
            visible = ((osd && !ended) || !live) && !inPip,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box(Modifier.fillMaxSize()) {
                Scrim(top = true, height = 120.dp, peak = 0.85f)
                Row(
                    Modifier.windowInsetsPadding(WindowInsets.displayCutout).padding(16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RoundButton(Icons.ArrowLeft, "Back", onBack)
                    Column(Modifier.weight(1f)) {
                        MqText(controller.title, role = TextRole.Title3, maxLines = 1)
                        controller.episode?.let {
                            MqText(it, role = TextRole.Callout, maxLines = 1)
                        }
                    }
                    if (live) RoundButton(Icons.PictureInPicture2, "Picture in picture", onPictureInPicture)
                }
            }
        }
        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Box {
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
                )
            }
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
                        note = if (controller.subtitlesPending) "Still asking subtitle addons" else null,
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
                                .padding(end = 16.dp, bottom = if (showControls) 200.dp else 24.dp),
                    )
                }
            }
            if (showNext && !ended && next != null) {
                UpNextCard(
                    next = next,
                    countdown = secondsLeft.takeIf { autoplayNext },
                    onPlay = { onNext(next, controller.bingeGroup) },
                    modifier =
                        Modifier
                            .align(Alignment.BottomEnd)
                            .windowInsetsPadding(WindowInsets.displayCutout)
                            .padding(end = 16.dp, bottom = 200.dp),
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
) {
    var dragging by remember { mutableStateOf<Long?>(null) }
    val playing = controller.offersPause
    Column(
        Modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.displayCutout)
            .padding(horizontal = 16.dp)
            .padding(bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        // The transport sits just above the bar and never crosses it.
        Row(
            Modifier.fillMaxWidth(),
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
        // Audio, Subtitles, Versions, Speed, then Episodes (the design audit's order for every platform).
        // Spread across the width when they fit; past it (a small phone, a large font, long track names)
        // the row scrolls, so no action is squeezed under its 48 dp target.
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            if (controller.audio.isNotEmpty()) {
                ActionButton(Icons.Volume2, "Audio", controller.audioDetail(), onAudio)
            }
            ActionButton(Icons.Captions, "Subtitles", controller.subtitlesDetail(), onSubtitles)
            ActionButton(Icons.Layers, "Versions", controller.versionLabel, onVersions)
            ActionButton(Icons.Gauge, "Speed", speedText(controller.speed), {
                onTouch()
                controller.changeSpeed(nextSpeed(controller.speed))
            })
            onEpisodes?.let { ActionButton(Icons.ListVideo, "Episodes", null, it) }
        }
    }
}
