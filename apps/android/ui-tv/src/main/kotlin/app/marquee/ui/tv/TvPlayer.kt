@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import android.text.format.DateFormat
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import app.marquee.core.model.BlockedReason
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PauseFilled
import app.marquee.design.PlayFilled
import app.marquee.design.RoundButton
import app.marquee.design.TextRole
import app.marquee.design.marqueeFocus
import app.marquee.player.ActionButton
import app.marquee.player.EndCard
import app.marquee.player.LoadingLayer
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
import app.marquee.player.TvSubtitlePicker
import app.marquee.player.UpNextCard
import app.marquee.player.activeSegment
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.endsAt
import app.marquee.player.loadingLines
import app.marquee.player.nextSpeed
import app.marquee.player.rememberTitleMark
import app.marquee.player.rememberTouchExploration
import app.marquee.player.seekStepMs
import app.marquee.player.speedText
import app.marquee.player.subtitlesDetail
import app.marquee.player.subtitlesStillListing
import app.marquee.player.upNextDue
import kotlinx.coroutines.flow.collectLatest
import java.util.Date
import kotlin.math.ceil

private const val HIDE_MS = 4_000L
private const val SEEK_MS = 10_000L

private enum class Panel { Audio, Subtitles }

private val selectKeys = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)

/**
 * The player on TV: the picture, and over it the title top left, the position bar and the
 * transport in the middle, and the actions (audio, subtitles, versions, episodes, speed) at the
 * right. The controls hide after four seconds of playing; while they are hidden the first key only
 * brings them back, with focus on play and pause. Left and right on the position bar step 10 s,
 * and a held key steps further (20, 30, 60 s) and lands when it is let go. Back closes a panel
 * first, then leaves.
 *
 * @param onNext the next episode was chosen (or it starts by itself when [autoplayNext] is on).
 * @param onEpisodes the title page of a series; null for a movie.
 */
@Composable
fun TvPlayer(
    controller: PlaybackController,
    autoplayNext: Boolean,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onNext: (NextEpisode, String?) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    /** False while the player fades out after leaving: Back and keys go to the page coming in. */
    active: Boolean = true,
    /** The failure card's "Play {label}": open that version from the same Versions view. */
    onOffer: (String) -> Unit = {},
    /** The subtitle picker's Size: the profile's subtitle size, for this video and the next. */
    onSubtitleSize: (SubtitleSize) -> Unit = {},
    /** Where a streaming service account is added: the failure card's way on when every version needs one. */
    onAddAccount: (() -> Unit)? = null,
) {
    val safe = Marquee.safe
    val stage = controller.stage
    val live = stage == Stage.Playing || stage == Stage.Paused || stage == Stage.Buffering || stage == Stage.Ended
    // Before the source shows a picture: the loading screen alone, nothing focused; the remote's Back cancels.
    val opening = controller.starting
    // The title's logo for the loading screen, loaded once for the play.
    val mark = rememberTitleMark(controller.logo)
    var osd by remember { mutableStateOf(true) }
    // The controls band's height (bar, transport and options), as last laid out: the cues sit over it.
    var controlsHeight by remember { mutableStateOf(0.dp) }
    val density = LocalDensity.current
    var wake by remember { mutableIntStateOf(0) }
    var panel by remember { mutableStateOf<Panel?>(null) }
    val ended = stage == Stage.Ended
    val showControls = osd && live && !ended && !opening
    val showEnd = ended && !(autoplayNext && controller.next != null)

    val root = remember { FocusRequester() }
    val play = remember { FocusRequester() }
    val scrub = remember { FocusRequester() }
    // Where focus goes when the controls come back, when not play and pause (the bar, after a seek).
    var enterOn by remember { mutableStateOf<FocusRequester?>(null) }
    val audioAction = remember { FocusRequester() }
    val subtitlesAction = remember { FocusRequester() }
    val failFocus = remember { FocusRequester() }
    val nextFocus = remember { FocusRequester() }
    val endFocus = remember { FocusRequester() }

    val next = controller.next
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
    val showSkip = segment != null && !showNext && panel == null && !ended && live && controller.status == null
    val skipFocus = remember { FocusRequester() }
    // Whether the Skip button has focus, so that when it goes only its own focus is handed back.
    var skipHasFocus by remember { mutableStateOf(false) }

    // Seconds to the end, once a second (the card's countdown), without recomposing on every tick.
    val secondsLeft by remember(controller) {
        derivedStateOf { ceil((controller.durationMs - controller.positionMs) / 1000.0).toInt().coerceAtLeast(0) }
    }

    // Controls hide while playing, once nothing is open and nothing is asking for an answer, and never
    // under TalkBack. Every key starts the wait again; `wake` is read here and not in composition, so a
    // key press recomposes nothing.
    val exploring = rememberTouchExploration()
    LaunchedEffect(osd, stage, panel, showNext, exploring) {
        if (osd && stage == Stage.Playing && panel == null && !showNext && !exploring) {
            snapshotFlow { wake }.collectLatest {
                kotlinx.coroutines.delay(HIDE_MS)
                osd = false
            }
        }
    }
    // Focus follows what is on screen: play and pause when the controls appear, the page itself
    // when they are gone (so keys still arrive), the way on when nothing plays.
    LaunchedEffect(showControls) {
        withFrameNanos { }
        runCatching { if (showControls) (enterOn ?: play).requestFocus() else root.requestFocus() }
        enterOn = null
    }
    LaunchedEffect(stage == Stage.Failed) {
        if (stage == Stage.Failed) {
            withFrameNanos { }
            runCatching { failFocus.requestFocus() }
        }
    }
    // The card takes focus when it appears, unless a list is open (choosing audio near the end must not be
    // interrupted); when it goes (a rewind), focus goes back to play and pause instead of being lost.
    LaunchedEffect(showNext) {
        if (showNext && panel == null) {
            osd = true
            withFrameNanos { }
            runCatching { nextFocus.requestFocus() }
        } else if (!showNext && panel == null && showControls) {
            withFrameNanos { }
            runCatching { play.requestFocus() }
        }
    }
    // Skip takes focus when it appears with the controls up; when it goes, focus goes back to play and pause.
    LaunchedEffect(showSkip) {
        withFrameNanos { }
        runCatching {
            if (showSkip && showControls) {
                skipFocus.requestFocus()
            } else if (!showSkip && skipHasFocus && showControls && panel == null && !showNext) {
                // Only when focus was on Skip: a viewer scrubbing the bar keeps it.
                play.requestFocus()
            }
        }
    }
    // A list does not stay open over a failure or any state without controls.
    LaunchedEffect(live) { if (!live) panel = null }
    // A list that was open when the film ended closes: the end card is the only thing to answer.
    LaunchedEffect(ended) { if (ended) panel = null }
    LaunchedEffect(showEnd) {
        if (showEnd) {
            withFrameNanos { }
            runCatching { endFocus.requestFocus() }
        }
    }
    // The end of an episode starts the next one when the viewer wants that.
    LaunchedEffect(stage) {
        if (stage == Stage.Ended && autoplayNext && next != null) onNext(next, controller.bingeGroup)
        if (stage == Stage.Ended) osd = true
    }

    // Closing a panel (Back, or a row chosen) returns focus to the action that opened it.
    var returnTo by remember { mutableStateOf<FocusRequester?>(null) }

    fun closePanel() {
        returnTo = if (panel == Panel.Audio) audioAction else subtitlesAction
        panel = null
        wake++
    }

    // When the last Back step was taken (event time), so a held Back steps at a steady pace.
    val lastBack = remember { longArrayOf(0L) }

    // Back, one step at a time (plans/android-design-v2.md 2.2, as Leanback does): an open panel
    // closes, then the controls hide, then the player leaves.
    fun back() {
        when {
            panel != null -> closePanel()
            showControls -> osd = false
            else -> onBack()
        }
    }
    // One handler that is always on (turning it on and off with the panel left Back ignored after a key press).
    BackHandler(enabled = active) { back() }
    LaunchedEffect(returnTo) {
        val target = returnTo ?: return@LaunchedEffect
        withFrameNanos { }
        runCatching { target.requestFocus() }
        returnTo = null
    }

    Box(
        modifier
            .fillMaxSize()
            .background(Color.Black)
            .focusRequester(root)
            .onPreviewKeyEvent { event ->
                if (!active || event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                when (event.key) {
                    Key.MediaPlayPause -> {
                        controller.togglePlay()
                        osd = true
                        wake++
                        true
                    }

                    Key.MediaPlay -> {
                        controller.play()
                        osd = true
                        wake++
                        true
                    }

                    Key.MediaPause -> {
                        controller.pause()
                        osd = true
                        wake++
                        true
                    }

                    Key.MediaRewind -> {
                        controller.seekBy(-SEEK_MS)
                        osd = true
                        wake++
                        true
                    }

                    Key.MediaFastForward -> {
                        controller.seekBy(SEEK_MS)
                        osd = true
                        wake++
                        true
                    }

                    Key.Back -> {
                        // Handled here while focus is in the player: on the emulator a Back that came
                        // after a D-pad press never reached the BackHandler below (it still covers
                        // the moments when no node has focus).
                        // A held Back takes the steps one at a time (2.2): a repeat acts once the last step is
                        // BACK_REPEAT_MS old, the rest are eaten.
                        val native = event.nativeKeyEvent
                        if (native.repeatCount == 0 || native.eventTime - lastBack[0] >= BACK_REPEAT_MS) {
                            lastBack[0] = native.eventTime
                            back()
                        }
                        true
                    }

                    else -> {
                        wake++
                        // While the controls are hidden the first key brings them back, except select while a
                        // Skip button shows (that presses it), and Left and Right: they seek at once, as on the
                        // Apple TV app, with the focus on the bar so a held key goes on scrubbing.
                        if (live && !osd) {
                            val skip = segment
                            val seek = hiddenSeekMs(event.key)
                            if (showSkip && skip != null && event.key in selectKeys) {
                                controller.skip(skip)
                            } else if (seek != null) {
                                controller.seekBy(seek)
                                enterOn = scrub
                                osd = true
                            } else {
                                osd = true
                            }
                            true
                        } else {
                            false
                        }
                    }
                }
            }.focusProperties { exit = { FocusRequester.Cancel } }
            .focusable(),
    ) {
        // While the controls show, the cues' foot sits 8 dp over their band, as on the phone.
        PlayerVideo(
            controller.player,
            Modifier.fillMaxSize(),
            controller.subtitleSize,
            subtitleLift = if (showControls) controlsHeight else 0.dp,
            addonCues = { controller.addonCues() }.takeIf { controller.addonOn },
        )

        // The loading screen until the picture comes, and over the paused picture while it stalls.
        val stalled = stage == Stage.Buffering && controller.picture
        if (opening || stalled) {
            LoadingLayer(
                title = controller.title,
                mark = mark,
                backdrop = controller.backdrop,
                fill = { controller.loadingFill },
                lines = loadingLines(controller.status, controller.start?.caption),
                opening = opening,
            )
        }

        // Scrims run edge to edge (no box with an edge to see): 320 dp at the foot to 72% black,
        // 200 dp at the top to 60%, so the words over bright pictures hold their contrast.
        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Scrim(top = false, height = 380.dp, peak = 0.85f)
        }
        AnimatedVisibility(visible = ((osd && !ended) || !live) && !opening, enter = fadeIn(), exit = fadeOut()) {
            Box(Modifier.fillMaxSize()) {
                Scrim(top = true, height = 200.dp, peak = 0.85f)
                Header(controller, Modifier.padding(start = safe.side, top = safe.top))
                EndsAt(controller, Modifier.align(Alignment.TopEnd).padding(end = safe.side, top = 70.dp))
            }
        }

        // Not recorded on Trakt: said under the title block, and still there when the controls hide.
        TraktWarning(
            controller.trakt,
            Modifier.align(Alignment.TopStart).padding(start = safe.side, top = safe.top + 150.dp),
        )

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
                onBack = onBack,
                focus = failFocus,
                modifier = Modifier.align(Alignment.TopStart).padding(start = safe.side, top = 360.dp),
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
                onAddAccount = onAddAccount?.takeIf { controller.blocked == BlockedReason.NeedsAccount },
            )
        }

        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Controls(
                controller = controller,
                play = play,
                scrub = scrub,
                audioAction = audioAction,
                subtitlesAction = subtitlesAction,
                onAudio = { panel = Panel.Audio },
                onSubtitles = { panel = Panel.Subtitles },
                onVersions = onVersions,
                onEpisodes = onEpisodes,
                modifier =
                    Modifier
                        .onSizeChanged { if (it.height > 0) controlsHeight = with(density) { it.height.toDp() } }
                        .padding(start = safe.side, end = safe.side, bottom = safe.bottom),
            )
        }

        when (panel) {
            Panel.Audio -> {
                TrackPanel(
                    title = "Audio",
                    rows = controller.audio,
                    onPick = {
                        controller.selectAudio(it)
                        closePanel()
                    },
                    modifier = Modifier.align(Alignment.BottomEnd).padding(end = safe.side, bottom = 300.dp),
                )
            }

            Panel.Subtitles -> {
                TvSubtitlePicker(
                    rows = controller.text,
                    onPick = controller::selectText,
                    modifier = Modifier.align(Alignment.TopEnd).padding(end = safe.side, top = 96.dp),
                    note = controller.subtitlePanelNote,
                    timing = controller.subtitleDelayMs.takeIf { controller.subtitleTimeable },
                    onDelay = controller::setSubtitleDelay,
                    textSize = controller.subtitleSize,
                    onSize = onSubtitleSize,
                )
            }

            null -> {
                Unit
            }
        }

        if (showEnd) {
            EndCard(
                title = controller.title,
                next = next,
                onNext = { next?.let { onNext(it, controller.bingeGroup) } },
                onAgain = controller::play,
                onBack = onBack,
                focus = endFocus,
                modifier = Modifier.fillMaxSize(),
            )
        }

        if (showSkip) {
            segment?.let { skip ->
                SkipButton(
                    segment = skip,
                    onSkip = { controller.skip(skip) },
                    focus = skipFocus,
                    onFocusChange = { skipHasFocus = it },
                    modifier =
                        Modifier
                            .align(Alignment.BottomEnd)
                            .padding(end = safe.side, bottom = if (showControls) 330.dp else 96.dp),
                )
            }
        }

        if (showNext && !ended && next != null) {
            UpNextCard(
                next = next,
                countdown = secondsLeft.takeIf { autoplayNext },
                onPlay = { onNext(next, controller.bingeGroup) },
                focus = nextFocus,
                modifier = Modifier.align(Alignment.BottomEnd).padding(end = safe.side, bottom = 330.dp),
            )
        }
    }
}

@Composable
private fun Header(
    controller: PlaybackController,
    modifier: Modifier,
) {
    Column(modifier.fillMaxWidth(0.7f)) {
        MqText(controller.title, role = TextRole.Title2, maxLines = 2)
        controller.episode?.let {
            MqText(it, role = TextRole.Body, modifier = Modifier.padding(top = 8.dp))
        }
        TraktMark(controller.trakt, Modifier.padding(top = 8.dp))
    }
}

/** "Ends at 9:41 PM", at the current speed; nothing until the length is known. */
@Composable
private fun EndsAt(
    controller: PlaybackController,
    modifier: Modifier,
) {
    val context = LocalContext.current
    val at =
        endsAt(System.currentTimeMillis(), controller.positionMs, controller.durationMs, controller.speed) ?: return
    MqText(
        "Ends at ${DateFormat.getTimeFormat(context).format(Date(at))}",
        modifier = modifier,
        role = TextRole.Callout,
        color = Marquee.colors.text,
    )
}

@Composable
private fun Controls(
    controller: PlaybackController,
    play: FocusRequester,
    scrub: FocusRequester,
    audioAction: FocusRequester,
    subtitlesAction: FocusRequester,
    onAudio: () -> Unit,
    onSubtitles: () -> Unit,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    modifier: Modifier,
) {
    var scrubTarget by remember { mutableStateOf<Long?>(null) }
    var scrubFocused by remember { mutableStateOf(false) }
    val playing = controller.offersPause
    Box(modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth()) {
            Row(horizontalArrangement = Arrangement.spacedBy(26.dp), verticalAlignment = Alignment.CenterVertically) {
                MqText(clockText(scrubTarget ?: controller.positionMs), role = TextRole.Callout)
                Scrubber(
                    position = { controller.positionMs },
                    duration = { controller.durationMs },
                    buffered = { controller.bufferedMs },
                    preview = { scrubTarget },
                    emphasized = scrubFocused,
                    onSeek = controller::seekTo,
                    modifier =
                        Modifier
                            .weight(1f)
                            .focusRequester(scrub)
                            .marqueeFocus(8.dp, scale = 1f)
                            .onFocusChanged {
                                scrubFocused = it.isFocused
                                if (!it.isFocused) scrubTarget = null
                            }.onKeyEvent { event ->
                                scrubKey(event, controller, scrubTarget) { scrubTarget = it }
                            }.focusable(),
                )
                MqText(
                    "-" + clockText((controller.durationMs - controller.positionMs).coerceAtLeast(0)),
                    role = TextRole.Callout,
                )
            }
            Spacer(Modifier.height(34.dp))
            // Three regions: the transport in the middle and the actions in the right one, so five
            // actions with long track names never cover Forward or Play. Actions that do not fit
            // their region scroll, and focus brings each into view.
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Spacer(Modifier.weight(1f))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(26.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RoundButton(Icons.RotateCcw, "Back 10 seconds", { controller.seekBy(-SEEK_MS) }, badge = "10")
                    RoundButton(
                        icon = if (playing) PauseFilled else PlayFilled,
                        label = if (playing) "Pause" else "Play",
                        onClick = controller::togglePlay,
                        main = true,
                        focusRequester = play,
                    )
                    RoundButton(Icons.RotateCw, "Forward 10 seconds", { controller.seekBy(SEEK_MS) }, badge = "10")
                }
                Box(Modifier.weight(1f), contentAlignment = Alignment.CenterEnd) {
                    Row(
                        Modifier
                            .horizontalScroll(rememberScrollState())
                            // Room for the focus halos at the scrolling edges.
                            .padding(horizontal = 16.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        // Audio, Subtitles, Versions, Speed, then Episodes: the design audit's order for every platform.
                        if (controller.audio.isNotEmpty()) {
                            ActionButton(
                                Icons.Volume2,
                                "Audio",
                                controller.audioDetail(),
                                onAudio,
                                focusRequester = audioAction,
                            )
                        }
                        ActionButton(
                            Icons.Captions,
                            "Subtitles",
                            controller.subtitlesDetail(),
                            onSubtitles,
                            focusRequester = subtitlesAction,
                            busy = controller.subtitlesStillListing(),
                        )
                        ActionButton(Icons.Layers, "Versions", controller.versionLabel, onVersions)
                        ActionButton(
                            Icons.Gauge,
                            "Speed",
                            speedText(controller.speed),
                            { controller.changeSpeed(nextSpeed(controller.speed)) },
                        )
                        onEpisodes?.let { ActionButton(Icons.ListVideo, "Episodes", null, it) }
                    }
                }
            }
        }
    }
}

/** Left and Right while the controls are hidden: ten seconds back or ahead. */
internal fun hiddenSeekMs(key: Key): Long? =
    when (key) {
        Key.DirectionLeft -> -SEEK_MS
        Key.DirectionRight -> SEEK_MS
        else -> null
    }

/** Left and right on the position bar: steps that grow while the key is held, committed when it is let go. */
private fun scrubKey(
    event: androidx.compose.ui.input.key.KeyEvent,
    controller: PlaybackController,
    target: Long?,
    set: (Long?) -> Unit,
): Boolean {
    val direction =
        when (event.key) {
            Key.DirectionLeft -> -1
            Key.DirectionRight -> 1
            else -> 0
        }
    if (direction == 0) {
        if (event.type == KeyEventType.KeyDown && (event.key == Key.DirectionCenter || event.key == Key.Enter)) {
            controller.togglePlay()
            return true
        }
        return false
    }
    return when (event.type) {
        KeyEventType.KeyDown -> {
            val step = seekStepMs(event.nativeKeyEvent.repeatCount)
            val end = controller.durationMs.takeIf { it > 0 } ?: Long.MAX_VALUE
            set(((target ?: controller.positionMs) + direction * step).coerceIn(0L, end))
            true
        }

        KeyEventType.KeyUp -> {
            target?.let(controller::seekTo)
            set(null)
            true
        }

        else -> {
            false
        }
    }
}

/** How often a held Back takes its next step. */
private const val BACK_REPEAT_MS = 400L
