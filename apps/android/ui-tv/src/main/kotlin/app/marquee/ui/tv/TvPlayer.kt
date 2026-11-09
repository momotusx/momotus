@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import android.text.format.DateFormat
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
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
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
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
import app.marquee.design.MqText
import app.marquee.design.PauseFilled
import app.marquee.design.PlayFilled
import app.marquee.design.RoundButton
import app.marquee.design.TextRole
import app.marquee.design.marqueeFocus
import app.marquee.player.ActionButton
import app.marquee.player.AudioStrip
import app.marquee.player.EndCard
import app.marquee.player.LoadingLayer
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import app.marquee.player.PlayerStatus
import app.marquee.player.PlayerVideo
import app.marquee.player.QuickShift
import app.marquee.player.Scrim
import app.marquee.player.Scrubber
import app.marquee.player.SkipButton
import app.marquee.player.Strip
import app.marquee.player.SubtitleStrip
import app.marquee.player.TvSubtitlePicker
import app.marquee.player.UpNextCard
import app.marquee.player.activeSegment
import app.marquee.player.audioDetail
import app.marquee.player.clockText
import app.marquee.player.endsAt
import app.marquee.player.footScrim
import app.marquee.player.loadingLines
import app.marquee.player.nextName
import app.marquee.player.nextSpeed
import app.marquee.player.rememberBarWidths
import app.marquee.player.rememberTitleMark
import app.marquee.player.rememberTouchExploration
import app.marquee.player.secondLine
import app.marquee.player.seekStepMs
import app.marquee.player.sideFit
import app.marquee.player.speedText
import app.marquee.player.stripLift
import app.marquee.player.subtitlesDetail
import app.marquee.player.subtitlesStillListing
import app.marquee.player.upNextDue
import kotlinx.coroutines.flow.collectLatest
import java.util.Date
import kotlin.math.ceil

private const val HIDE_MS = 4_000L
private const val SEEK_MS = 10_000L

private val selectKeys = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)

private val dpadKeys =
    setOf(Key.DirectionUp, Key.DirectionDown, Key.DirectionLeft, Key.DirectionRight, Key.DirectionCenter, Key.Enter)

/**
 * The player on TV: the picture, and over it the title top left; at the foot the position bar, then one row
 * (plans/player-bar.md): Audio, Speed and Versions at the left, the transport in the middle, Subtitles with its quick
 * shift, Episodes and Next at the right, at the picked bar's size times 1.5 (D19). Subtitles and Audio open their
 * strips over the bar's ends. The controls hide after four seconds of playing; while they are hidden the first key
 * only brings them back, with focus on play and pause. Left and right on the position bar step 10 s, and a held key
 * steps further (20, 30, 60 s) and lands when it is let go. Back leaves the shift's adjust mode, then closes the
 * Style row, a strip or the picker, then hides the controls, then leaves (section 6).
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
    /** The picker's second line: the profile's layout, Only when paused, and the Spoken language kept with no track here. */
    onSubtitleLayout: (SubtitleLayout) -> Unit = {},
    onSecondOnPause: (Boolean) -> Unit = {},
    onSpokenKept: () -> Unit = {},
    /** The failure card's Settings: the addons (noSource). */
    onSettings: ((EmptyKind) -> Unit)? = null,
    /** The strip's Style: the profile's subtitle position and background, as Size. */
    onSubtitlePosition: (Int) -> Unit = {},
    onSubtitleBackground: (SubtitleBackground) -> Unit = {},
) {
    val safe = Marquee.safe
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
    val live = stage == Stage.Playing || stage == Stage.Paused || stage == Stage.Buffering || stage == Stage.Ended
    // Before the source shows a picture: the loading screen alone, nothing focused; the remote's Back cancels.
    val opening = controller.starting
    // The title's logo for the loading screen, loaded once for the play.
    val mark = rememberTitleMark(controller.logo)
    var osd by remember { mutableStateOf(true) }
    // The controls band's height (bar, transport and options), as last laid out: the cues sit over it.
    var controlsHeight by remember { mutableStateOf(0.dp) }
    // The open strip's height, with its Style row, as last laid out.
    var stripHeight by remember { mutableStateOf(0.dp) }
    // The title block's foot, as last laid out: Split's second line hangs under it while the controls show.
    var headerFoot by remember { mutableStateOf(0.dp) }
    val density = LocalDensity.current
    var wake by remember { mutableIntStateOf(0) }
    // The full subtitle picker (from More languages); the strip over the bar, its Style row, the shift's adjust mode.
    var picker by remember { mutableStateOf(false) }
    var strip by remember { mutableStateOf<Strip?>(null) }
    var style by remember { mutableStateOf(false) }
    var adjusting by remember { mutableStateOf(false) }
    val ended = stage == Stage.Ended
    val showControls = osd && live && !ended && !opening
    // The loading screen over the paused picture while it waits for data.
    val stalled = stage == Stage.Buffering && controller.picture
    // Choose another version when the core offers it, on the loading screen (20 s into the start) and over a stalled
    // frame (30 s): one clock, the core's, for every kind of source (plans/versions-android-spec.md, 6).
    val chooseShown = (opening || stalled) && controller.chooseAnother
    val chooseFocus = remember { FocusRequester() }
    var chooseFocused by remember { mutableStateOf(false) }
    // Cancel on Up Next stops the next episode from starting by itself, for this playback: the end card shows instead.
    var upNextCancelled by remember(controller) { mutableStateOf(false) }
    val autoplay = autoplayNext && !upNextCancelled
    val showEnd = ended && !(autoplay && controller.next != null)

    val root = remember { FocusRequester() }
    val play = remember { FocusRequester() }
    val scrub = remember { FocusRequester() }
    // Where focus goes when the controls come back, when not play and pause (the bar, after a seek).
    var enterOn by remember { mutableStateOf<FocusRequester?>(null) }
    val audioAction = remember { FocusRequester() }
    val subtitlesAction = remember { FocusRequester() }
    // The open strip's checked chip (else its first): where a remote enters the strip.
    val stripFirst = remember { FocusRequester() }
    val failFocus = remember { FocusRequester() }
    val nextFocus = remember { FocusRequester() }
    val endFocus = remember { FocusRequester() }

    val next = controller.next
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
    val showSkip = segment != null && !showNext && !picker && !ended && live && controller.status == null
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
    LaunchedEffect(osd, stage, picker, strip, adjusting, showNext, exploring) {
        if (osd && stage == Stage.Playing && !picker && strip == null && !adjusting && !showNext && !exploring) {
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
        if (showNext && !picker) {
            osd = true
            withFrameNanos { }
            runCatching { nextFocus.requestFocus() }
        } else if (!showNext && !picker && showControls) {
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
            } else if (!showSkip && skipHasFocus && showControls && !picker && !showNext) {
                // Only when focus was on Skip: a viewer scrubbing the bar keeps it.
                play.requestFocus()
            }
        }
    }

    fun dropStrip() {
        strip = null
        style = false
        adjusting = false
    }
    // A list or strip does not stay open over a failure, the end card or any state without controls.
    LaunchedEffect(live, ended) {
        if (!live || ended) {
            picker = false
            dropStrip()
        }
    }
    // Skip and Up Next carry a time-bound action: an open strip gives them its place and the focus (D23).
    LaunchedEffect(showSkip || showNext) { if (showSkip || showNext) dropStrip() }
    LaunchedEffect(showEnd) {
        if (showEnd) {
            withFrameNanos { }
            runCatching { endFocus.requestFocus() }
        }
    }
    // The end of an episode starts the next one when the viewer wants that.
    LaunchedEffect(stage) {
        if (stage == Stage.Ended && autoplay && next != null) onNext(next, controller.bingeGroup)
        if (stage == Stage.Ended) osd = true
    }

    // Closing the picker or a strip returns focus to the pill that opened it.
    var returnTo by remember { mutableStateOf<FocusRequester?>(null) }

    fun closeStrip() {
        returnTo = if (strip == Strip.Audio) audioAction else subtitlesAction
        dropStrip()
        wake++
    }

    fun openStrip(opening: Strip) {
        if (strip == opening) return closeStrip()
        style = false
        adjusting = false
        strip = opening
        wake++
    }
    // A strip opened by remote takes the focus on its checked chip, else its first (section 6).
    LaunchedEffect(strip) {
        if (strip != null) {
            withFrameNanos { }
            withFrameNanos { }
            runCatching { stripFirst.requestFocus() }
        }
    }

    // When the last Back step was taken (event time), so a held Back steps at a steady pace.
    val lastBack = remember { longArrayOf(0L) }

    // Back, one step at a time (section 6, as Leanback does): the shift's adjust mode, the Style row, the strip,
    // the picker, then the controls hide, then the player leaves.
    fun back() {
        when {
            adjusting -> {
                adjusting = false
            }

            style -> {
                style = false
            }

            strip != null -> {
                closeStrip()
            }

            picker -> {
                returnTo = subtitlesAction
                picker = false
                wake++
            }

            showControls -> {
                osd = false
            }

            else -> {
                onBack()
            }
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
                // The button never takes focus by itself: the first D-pad key after it shows moves there.
                if ((opening || stalled) && chooseShown && !chooseFocused && event.key in dpadKeys) {
                    runCatching { chooseFocus.requestFocus() }
                    return@onPreviewKeyEvent true
                }
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
        // While the controls show, the cues' foot sits 8 dp over their band, or over an open strip, as on the phone.
        val stripCover = if (strip != null) stripHeight + stripLift() else 0.dp
        PlayerVideo(
            controller.player,
            Modifier.fillMaxSize(),
            controller.subtitleSize,
            subtitleLift = if (showControls) controlsHeight + stripCover else 0.dp,
            addonCues = { controller.addonCues() }.takeIf { controller.addonOn },
            videoRatio = controller.videoRatio,
            secondCues = { controller.secondCues() }.takeIf { controller.secondOn },
            subtitleLayout = controller.subtitleLayout,
            secondOnPause = controller.secondOnPause,
            subtitleTop = if (showControls) headerFoot else 0.dp,
            subtitlePosition = controller.subtitlePosition,
            subtitleBackground = controller.subtitleBackground,
        )

        // The loading screen until the picture comes, and over the paused picture while it stalls.
        if (opening || stalled) {
            LoadingLayer(
                title = controller.title,
                mark = mark,
                backdrop = controller.backdrop,
                fill = { controller.loadingFill },
                lines = loadingLines(controller.status, controller.start?.caption),
                opening = opening,
                choose = onVersions.takeIf { opening || stalled },
                chooseShown = chooseShown,
                chooseFocus = chooseFocus,
                chooseFocusChange = { chooseFocused = it },
            )
        }

        // Scrims run edge to edge (no box with an edge to see), so the words over bright pictures hold their
        // contrast; the foot's top follows an open strip (section 5, scrim).
        AnimatedVisibility(
            visible = showControls,
            modifier = Modifier.align(Alignment.BottomCenter),
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            Scrim(top = false, height = footScrim(controlsHeight + stripCover, 300.dp), peak = 0.85f)
        }
        AnimatedVisibility(visible = ((osd && !ended) || !live) && !opening, enter = fadeIn(), exit = fadeOut()) {
            Box(Modifier.fillMaxSize()) {
                Scrim(top = true, height = 200.dp, peak = 0.85f)
                Header(
                    controller,
                    Modifier
                        .padding(start = safe.side, top = safe.top)
                        .onGloballyPositioned { headerFoot = with(density) { it.boundsInRoot().bottom.toDp() } },
                )
                EndsAt(controller, Modifier.align(Alignment.TopEnd).padding(end = safe.side, top = 70.dp))
            }
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
                empty = controller.empty,
                kept = controller.keptKey,
                onSettings =
                    onSettings?.let { go ->
                        { kind: EmptyKind ->
                            controller.askAgain = true
                            go(kind)
                        }
                    },
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
                stripFirst = stripFirst,
                strip = strip,
                onStrip = ::openStrip,
                onVersions = onVersions,
                onEpisodes = onEpisodes,
                onNext = next?.let { { onNext(it, controller.bingeGroup) } },
                adjusting = adjusting,
                onAdjust = { adjusting = it },
                modifier =
                    Modifier
                        .onSizeChanged { if (it.height > 0) controlsHeight = with(density) { it.height.toDp() } }
                        .padding(start = safe.side, end = safe.side, bottom = safe.bottom),
            )
        }

        // The strips over the bar's ends (section 5); Down from one goes back to its pill.
        if (showControls) {
            val placed =
                Modifier
                    .padding(start = safe.side, end = safe.side, bottom = controlsHeight + stripLift())
                    .onSizeChanged { stripHeight = with(density) { it.height.toDp() } }
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
                            dropStrip()
                            picker = true
                        },
                        onClose = ::closeStrip,
                        onSize = onSubtitleSize,
                        onPosition = onSubtitlePosition,
                        onBackground = onSubtitleBackground,
                        modifier = Modifier.align(Alignment.BottomEnd).then(placed),
                        first = stripFirst,
                        opener = subtitlesAction,
                        adjusting = adjusting,
                        onAdjust = { adjusting = it },
                    )
                }

                Strip.Audio -> {
                    AudioStrip(
                        controller,
                        onVersions = onVersions,
                        onClose = ::closeStrip,
                        modifier = Modifier.align(Alignment.BottomStart).then(placed),
                        first = stripFirst,
                        opener = audioAction,
                    )
                }

                null -> {
                    Unit
                }
            }
        }

        if (picker) {
            TvSubtitlePicker(
                rows = controller.text,
                onPick = controller::selectText,
                modifier = Modifier.align(Alignment.TopEnd).padding(end = safe.side, top = 96.dp),
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
                            .padding(end = safe.side, bottom = if (showControls) controlsHeight + CARD_GAP else 96.dp),
                )
            }
        }

        // Over the bar as it is laid out (it shows while the card does).
        if (showNext && !ended && next != null) {
            UpNextCard(
                next = next,
                countdown = secondsLeft.takeIf { autoplay },
                onPlay = { onNext(next, controller.bingeGroup) },
                onCancel = { upNextCancelled = true },
                focus = nextFocus,
                modifier =
                    Modifier
                        .align(Alignment.BottomEnd)
                        .padding(end = safe.side, bottom = controlsHeight + CARD_GAP),
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

/**
 * The bar on TV (plans/player-bar.md, D16 and D19): the position bar, then one row of three columns, the pick's
 * `1fr auto 1fr`: Audio, Speed and Versions start the left one, the transport takes the middle, Subtitles, the quick
 * shift, Episodes and Next end the right one. A side whose pills do not fit its column with their words shows them as
 * icons ([sideFit]), and then the shift as chevrons. Up from the row goes to the position bar, or into an open strip
 * from the pills under it; Down from the position bar goes to Play (section 6).
 */
@Composable
private fun Controls(
    controller: PlaybackController,
    play: FocusRequester,
    scrub: FocusRequester,
    audioAction: FocusRequester,
    subtitlesAction: FocusRequester,
    stripFirst: FocusRequester,
    strip: Strip?,
    onStrip: (Strip) -> Unit,
    onVersions: () -> Unit,
    onEpisodes: (() -> Unit)?,
    onNext: (() -> Unit)?,
    adjusting: Boolean,
    onAdjust: (Boolean) -> Unit,
    modifier: Modifier,
) {
    var scrubTarget by remember { mutableStateOf<Long?>(null) }
    var scrubFocused by remember { mutableStateOf(false) }
    val playing = controller.offersPause
    val names = StreamChoiceCopy.Bar.Names

    // Up: into the strip open over this side, else to the position bar.
    fun under(side: Strip) = Modifier.focusProperties { up = if (strip == side) stripFirst else scrub }
    val upLeft = under(Strip.Audio)
    val upRight = under(Strip.Subtitles)
    Box(modifier.fillMaxWidth().testTag(TV_BAR_TAG)) {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(15.dp)) {
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
                            .focusProperties {
                                down = play
                                if (strip != null) up = stripFirst
                            }.marqueeFocus(8.dp, scale = 1f)
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
            val widths = rememberBarWidths()
            val subtitles = controller.subtitlesDetail()
            val audio = controller.audioDetail()
            val speed = speedText(controller.speed)
            val version = controller.versionLabel
            BoxWithConstraints(Modifier.fillMaxWidth()) {
                // Each side's column: half of what the transport and the two gaps leave (D16).
                val column = (maxWidth - TRANSPORT_WIDTH - COLUMN_GAP * 2) / 2
                val pill = widths.pill
                val left =
                    sideFit(
                        column,
                        words =
                            widths.pill(names.AUDIO, audio) + widths.pill(names.SPEED, speed) +
                                widths.pill(names.VERSIONS, version) + PILL_GAP * 2,
                        icons = pill * 3 + PILL_GAP * 2,
                    )
                val rightPills = 1 + (if (onEpisodes != null) 2 else 0)
                val right =
                    sideFit(
                        column,
                        words =
                            widths.pill(names.SUBTITLES, subtitles) +
                                (if (onEpisodes != null) widths.pill(names.EPISODES, null) else 0.dp) +
                                (if (onEpisodes != null) widths.pill(StreamChoiceCopy.Bar.NEXT, null) else 0.dp) +
                                PILL_GAP * rightPills,
                        icons = pill * rightPills + PILL_GAP * rightPills,
                        shiftWords = widths.shiftWords,
                        shiftChevrons = widths.shiftChevrons,
                    )
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(COLUMN_GAP),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(
                        Modifier.weight(1f),
                        horizontalArrangement = Arrangement.spacedBy(PILL_GAP),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // Audio once the file's tracks are known, as before.
                        if (controller.audio.isNotEmpty()) {
                            ActionButton(
                                Icons.AudioLines,
                                names.AUDIO,
                                audio,
                                { onStrip(Strip.Audio) },
                                upLeft,
                                focusRequester = audioAction,
                                words = left.words,
                                expanded = strip == Strip.Audio,
                            )
                        }
                        ActionButton(
                            Icons.Gauge,
                            names.SPEED,
                            speed,
                            { controller.changeSpeed(nextSpeed(controller.speed)) },
                            upLeft,
                            words = left.words,
                        )
                        ActionButton(Icons.Layers, names.VERSIONS, version, onVersions, upLeft, words = left.words)
                    }
                    Row(
                        Modifier.width(TRANSPORT_WIDTH),
                        horizontalArrangement = Arrangement.spacedBy(TRANSPORT_GAP, Alignment.CenterHorizontally),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        val toScrub = Modifier.focusProperties { up = scrub }
                        RoundButton(
                            Icons.RotateCcw,
                            "Back 10 seconds",
                            { controller.seekBy(-SEEK_MS) },
                            toScrub,
                            badge = "10",
                            diameter = pill,
                        )
                        RoundButton(
                            icon = if (playing) PauseFilled else PlayFilled,
                            label = if (playing) "Pause" else "Play",
                            onClick = controller::togglePlay,
                            modifier = toScrub,
                            main = true,
                            focusRequester = play,
                            diameter = PLAY,
                        )
                        RoundButton(
                            Icons.RotateCw,
                            "Forward 10 seconds",
                            { controller.seekBy(SEEK_MS) },
                            toScrub,
                            badge = "10",
                            diameter = pill,
                        )
                    }
                    Row(
                        Modifier.weight(1f),
                        horizontalArrangement = Arrangement.spacedBy(PILL_GAP, Alignment.End),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        ActionButton(
                            Icons.Captions,
                            names.SUBTITLES,
                            subtitles,
                            { onStrip(Strip.Subtitles) },
                            upRight,
                            focusRequester = subtitlesAction,
                            busy = controller.subtitlesStillListing(),
                            words = right.words,
                            expanded = strip == Strip.Subtitles,
                        )
                        // Only while the subtitle on can be timed (D4): the value is the one focus stop.
                        controller.quickShift?.let { delay ->
                            QuickShift(
                                delay,
                                controller::setSubtitleDelay,
                                words = right.shiftWords,
                                valueFocus = remember { FocusRequester() },
                                adjusting = adjusting,
                                onAdjust = onAdjust,
                                valueModifier = upRight,
                            )
                        }
                        onEpisodes?.let {
                            ActionButton(Icons.ListVideo, names.EPISODES, null, it, upRight, words = right.words)
                        }
                        val next = controller.next
                        if (next != null && onNext != null) {
                            ActionButton(
                                Icons.SkipForward,
                                StreamChoiceCopy.Bar.NEXT,
                                null,
                                onNext,
                                upRight,
                                words = right.words,
                                spoken = nextName(next),
                            )
                        }
                    }
                }
            }
        }
    }
}

/** The bar on TV, for tests. */
internal const val TV_BAR_TAG = "tv-player-bar"

/** The TV bar's sizes on the 1920-point canvas: the picked frame's at 1.5 times (D19). */
private val PLAY = 84.dp
private val TRANSPORT_GAP = 21.dp
private val TRANSPORT_WIDTH = 72.dp * 2 + PLAY + TRANSPORT_GAP * 2
private val PILL_GAP = 15.dp
private val COLUMN_GAP = 24.dp

/** Between the bar and the Skip button or the Up Next card over it. */
private val CARD_GAP = 24.dp

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
