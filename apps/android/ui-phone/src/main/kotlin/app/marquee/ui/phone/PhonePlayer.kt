package app.marquee.ui.phone

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import app.marquee.design.TextRole
import app.marquee.player.ActionButton
import app.marquee.player.EndCard
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import app.marquee.player.PlayerStatus
import app.marquee.player.PlayerTheme
import app.marquee.player.PlayerVideo
import app.marquee.player.RoundButton
import app.marquee.player.Scrim
import app.marquee.player.Scrubber
import app.marquee.player.Spinner
import app.marquee.player.TrackPanel
import app.marquee.player.UpNextCard
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.nextSpeed
import app.marquee.player.speedText
import app.marquee.player.subtitlesDetail
import kotlin.math.ceil

private const val HIDE_MS = 4_000L
private const val NEXT_AT_MS = 20_000L
private const val NEXT_MIN_LENGTH_MS = 60_000L
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
    PlayerTheme {
        val stage = controller.stage
        val live = stage == Stage.Playing || stage == Stage.Paused || stage == Stage.Buffering || stage == Stage.Ended
        var osd by remember { mutableStateOf(true) }
        var wake by remember { mutableIntStateOf(0) }
        var panel by remember { mutableStateOf<Panel?>(null) }
        var hint by remember { mutableStateOf<Int?>(null) }
        val ended = stage == Stage.Ended
        val showControls = osd && live && !ended && !inPip
        val next = controller.next
        val showEnd = ended && !(autoplayNext && next != null)
        val showNext by remember(controller) {
            derivedStateOf {
                val duration = controller.durationMs
                controller.next != null && duration > NEXT_MIN_LENGTH_MS &&
                    duration - controller.positionMs <= NEXT_AT_MS
            }
        }

        LaunchedEffect(osd, wake, stage, panel, showNext) {
            if (osd && stage == Stage.Playing && panel == null && !showNext) {
                kotlinx.coroutines.delay(HIDE_MS)
                osd = false
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
                            osd = !osd
                            wake++
                        },
                        onDoubleTap = { at ->
                            val forward = at.x > size.width / 2
                            controller.seekBy(if (forward) SEEK_MS else -SEEK_MS)
                            hint = if (forward) 1 else -1
                        },
                    )
                },
        ) {
            PlayerVideo(controller.player, Modifier.fillMaxSize())

            if (!inPip) {
                hint?.let { side ->
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
                    Scrim(top = true, height = 120.dp, alpha = 0.6f)
                    Row(
                        Modifier.windowInsetsPadding(WindowInsets.displayCutout).padding(16.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RoundButton(Icons.ArrowLeft, "Back", onBack)
                        Column(Modifier.weight(1f)) {
                            MqText(controller.title, role = TextRole.Title3, maxLines = 1)
                            controller.episode?.let {
                                MqText(it, role = TextRole.Callout, color = Marquee.colors.muted, maxLines = 1)
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
                        height = 200.dp,
                        alpha = 0.72f,
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
                            note = if (controller.subtitlesPending) "Still asking subtitle add-ons" else null,
                        )
                    }

                    null -> {
                        Unit
                    }
                }
                if (showNext && !ended && next != null) {
                    val remaining =
                        ceil(
                            (controller.durationMs - controller.positionMs) / 1000.0,
                        ).toInt().coerceAtLeast(0)
                    UpNextCard(
                        next = next,
                        countdown = remaining.takeIf { autoplayNext },
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
                icon = if (playing) Icons.Pause else Icons.Play,
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
                modifier =
                    Modifier
                        .weight(1f)
                        .height(40.dp)
                        .pointerInput(Unit) {
                            detectTapGestures { at ->
                                onTouch()
                                controller.seekTo((at.x / size.width * controller.durationMs).toLong())
                            }
                        }.pointerInput(Unit) {
                            detectHorizontalDragGestures(
                                onDragStart = { at -> dragging = (at.x / size.width * controller.durationMs).toLong() },
                                onHorizontalDrag = { change, _ ->
                                    onTouch()
                                    dragging =
                                        (change.position.x / size.width * controller.durationMs).toLong().coerceIn(
                                            0L,
                                            controller.durationMs,
                                        )
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
        // Audio, Subtitles, Versions, Speed, then Episodes: the same order on every platform.
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            if (controller.audio.isNotEmpty()) {
                ActionButton(Icons.Volume2, "Audio", controller.audioDetail(), onAudio)
            }
            ActionButton(Icons.Captions, "Subtitles", controller.subtitlesDetail(), onSubtitles)
            ActionButton(Icons.Layers, "Versions", null, onVersions)
            ActionButton(Icons.Gauge, "Speed", speedText(controller.speed), {
                onTouch()
                controller.changeSpeed(nextSpeed(controller.speed))
            })
            onEpisodes?.let { ActionButton(Icons.ListVideo, "Episodes", null, it) }
        }
    }
}
