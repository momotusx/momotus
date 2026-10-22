package app.marquee.player

import android.os.SystemClock
import androidx.annotation.VisibleForTesting
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.core.net.toUri
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.text.Cue
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlaybackException
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Attempt
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.OtherVersion
import app.marquee.core.model.PlaybackPhase
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.Segment
import app.marquee.core.model.StartProgress
import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleSize
import app.marquee.core.model.SubtitleTrack
import app.marquee.core.model.TraktPlayback
import app.marquee.core.views.PlaybackView
import app.marquee.core.views.ViewStatus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The glue between the core's `playback` view and the ExoPlayer, and the one state both OSDs read.
 * Main thread only.
 *
 * - The core says where to play from (`ready`, a loopback URL, the saved position); the controller
 *   gives it to the player and raises `playWhenReady` at the first `READY`, so sound never runs
 *   ahead of the picture.
 * - Progress goes to the core about every 5 s while playing, on pause, after a seek settles, at the
 *   end and on release, never before a frame was shown (see [progressReport]).
 * - A playback error stops the player first and only then tells the core (`failOver`), because the
 *   core answers with another source and a late error from the dead one must not fail the new one;
 *   the new source starts at the position the old one reached.
 * - The core's (addon) subtitles are drawn over the picture from their own files ([addonCues]); the file's own
 *   go through track selection and draw with their delay ([DelayedTextRenderer]). A pick, Off or a delay step
 *   never sets the media item again.
 *
 * @param openedAtMs [SystemClock.elapsedRealtime] when Play was chosen, for the time to first frame.
 */
class PlaybackController(
    val player: Player,
    private val scope: CoroutineScope,
    val title: String,
    /** "S1:E2", or null for a movie. */
    val episode: String?,
    private val artwork: String?,
    private val openedAtMs: Long,
    /** The delay the file's own tracks draw with ([PlayerHost.subtitleDelay]). */
    private val subtitleDelay: SubtitleDelay = SubtitleDelay(),
    /** An addon subtitle file from its loopback address ([fetchSubtitle]). */
    private val fetch: suspend (String) -> ByteArray = ::fetchSubtitle,
) {
    enum class Stage { Opening, Buffering, Playing, Paused, Ended, Failed }

    var stage by mutableStateOf(Stage.Opening)
        private set

    /** What is going on while nothing plays yet or the source changes ("Starting 1080p from Local Test"). */
    var status: String? by mutableStateOf(null)
        private set

    /** A line said for a few seconds over the picture (a subtitle note, a source change): nothing is waited for. */
    var notice: String? by mutableStateOf(null)
        private set

    /** Why nothing plays, when [stage] is [Stage.Failed]. */
    var problem: String? by mutableStateOf(null)
        private set

    var attempts: List<Attempt> by mutableStateOf(emptyList())
        private set

    /** A version picked by hand failed for a reason of its own: the failure card offers this one first. */
    var offer: OtherVersion? by mutableStateOf(null)
        private set

    /** The core's play failed: the failure card offers Try again ([retry]). */
    var retryable by mutableStateOf(false)
        private set

    /** Why the version could not be opened, under the card's headline ([cannotOpen]); null otherwise. */
    var openError: String? by mutableStateOf(null)
        private set

    /** Opens the version again after it could not be opened: the card's Try again ([reopen]). */
    private var opener: (() -> Unit)? by mutableStateOf(null)

    /** The card offers Try again for a version that could not be opened. */
    val reopenable: Boolean get() = opener != null

    /** The core took a Try again and the play failed again at once: the card says so for a moment. */
    var tryingAgain by mutableStateOf(false)
        private set

    /** The core's count of Try again taken (`PlaybackSnapshot.retries`), from the first snapshot on. */
    private var retriesSeen: Int? = null
    private var tryingAgainJob: Job? = null

    var positionMs by mutableLongStateOf(0L)
        private set

    var durationMs by mutableLongStateOf(0L)
        private set

    var bufferedMs by mutableLongStateOf(0L)
        private set

    var speed by mutableFloatStateOf(1f)
        private set

    var audio: List<TrackChoice> by mutableStateOf(emptyList())
        private set

    var text: List<TrackChoice> by mutableStateOf(listOf(TrackChoice(TRACK_OFF, "Off", selected = true)))
        private set

    var subtitlesPending by mutableStateOf(false)
        private set

    /** Subtitle sources that gave nothing this time (plans/m13-turkcealtyazi.md 4.2), in the core's order. */
    var subtitleNotes by mutableStateOf<List<SubtitleNote>>(emptyList())
        private set

    /** Under the subtitle list: the pending line, then the notes. */
    val subtitlePanelNote: String? get() = subtitlePanelNote(subtitlesPending, subtitleNotes)

    var next: NextEpisode? by mutableStateOf(null)
        private set

    /** Pass to Versions of the next episode so the same release line is picked. */
    var bingeGroup: String? = null
        private set

    /** Skip segments the core found for the playing version (intro, recap, credits, preview); empty while unknown. */
    @set:VisibleForTesting(otherwise = VisibleForTesting.PRIVATE)
    var segments: List<Segment> by mutableStateOf(emptyList())

    /** The position the core says the Up Next card is due (the credits start); null means the last seconds. */
    var nextAtMs: Long? by mutableStateOf(null)
        private set

    /** Trakt's side of this play, for the player's mark; null until the core says. */
    var trakt: TraktPlayback? by mutableStateOf(null)
        private set

    /** Where Skip credits was pressed: Up Next shows at once from there, until the viewer seeks back before it. */
    var skippedAtMs: Long? by mutableStateOf(null)
        private set

    /**
     * The loading screen's fill, 0 to 1 (plans/player-polish.md 1.2): the core's start progress while it resolves,
     * then the player's buffer against its start threshold; on a stall, the buffer against the rebuffer one. Never
     * back within one start.
     */
    var loadingFill by mutableFloatStateOf(0f)
        private set

    /** The core's start progress while it resolves (`PlaybackSnapshot.start`): its caption names a long wait. */
    var start: StartProgress? by mutableStateOf(null)
        private set

    /** The source that plays has shown a frame: a wait from here on is a stall over the paused picture. */
    var picture by mutableStateOf(false)
        private set

    /** The start is under way: the core resolves or the player buffers, and this source has shown no picture yet. */
    val starting: Boolean get() = (stage == Stage.Opening || stage == Stage.Buffering) && !picture

    /** The app went away while the start ran: it went on paused, and plays once the viewer is back ([onStart]). */
    private var playOnReturn = false

    /** The title's backdrop and logo for the loading screen; the screen sets them, as it knows the title. */
    var backdrop: String? by mutableStateOf(null)
    var logo: String? by mutableStateOf(null)

    /** Where the fill stood when the player got the URL: the buffer fills the rest. */
    private var fillBase = 0f

    /** Width over height of the picture once known; 0 before. */
    var videoRatio by mutableFloatStateOf(0f)
        private set

    var firstFrameMs: Long? by mutableStateOf(null)
        private set

    /** The viewer wants it playing (it is not paused by them). */
    var wantPlay by mutableStateOf(true)
        private set

    /** The transport button offers Pause: playing, or buffering after a seek while the viewer wants it playing. */
    val offersPause: Boolean get() = stage == Stage.Playing || (stage == Stage.Buffering && wantPlay)

    /** The key of the version that is playing (it changes on a failover). */
    var playingKey: String? by mutableStateOf(null)
        private set

    /** The core's attempt number of the source loaded, named when it fails. */
    private var playingAttempt: Long? = null

    /** The quality of the version that plays ("1080p"), the Versions button's value; the screen sets it, as it knows the ranking. */
    var versionLabel: String? by mutableStateOf(null)

    /** How big subtitles show, from the profile's settings; the screen sets it. */
    var subtitleSize: SubtitleSize by mutableStateOf(SubtitleSize.Medium)

    /**
     * The chosen track's subtitle delay (positive: later), from the subtitle menu's Timing row. Each track keeps its
     * own for the rest of the play ([delays]); a track not timed yet starts at zero.
     */
    var subtitleDelayMs by mutableLongStateOf(0L)
        private set

    /** Each track's delay in this play, by [delayKey]. */
    private val delays = mutableMapOf<String, Long>()

    private var view: PlaybackView? = null
    private var loadedUrl: String? = null

    /** The addon track drawn over the picture, once its file is in; null with none on. */
    private var shown: TimedCues? by mutableStateOf(null)

    /** Addon files already parsed in this play: a pick back to one shows at once. */
    private val fetched = HashMap<String, TimedCues>()
    private var fetching: Job? = null
    private var snapshotSubs: List<SubtitleTrack> = emptyList()
    private var firstFrame = false
    private var raised = false
    private var seekPending = false

    /** When the buffer ran dry during play, 0 when it did not: the race counts stalls (plans/m13-stream-choice.md 6.4). */
    private var stalledAt = 0L
    private var handlingFailure = false
    private var lastPositionMs = 0L
    private var pick: TextPick? = null
        set(value) {
            field = value
            // A delay the viewer set belongs to the track it was set on: it goes to the core before another comes.
            if (value?.url != unsentDelay?.first) sendDelay()
            // A track comes on with its own delay (a core track where the viewer last left it); Off keeps the last
            // one, as nothing shows.
            if (value != null) applyDelay(delays[value.delayKey] ?: startDelay(value))
        }

    /** The viewer picked a subtitle or Off in this play: the core's start subtitle no longer turns on. */
    private var chose = false

    /** The start subtitle already turned on, so a repeat of it does nothing. */
    private var started: String? = null

    /** Addon subtitle files that did not load in this play: not offered again. */
    private val dead = HashSet<String>()
    private var released = false
    private var durationSentFor: String? = null
    private var ticker: Job? = null
    private var noticeJob: Job? = null
    private val marks = LinkedHashMap<String, Long>()
    private val pictureWait = PictureWait()

    private val listener =
        object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) = stateChanged(playbackState)

            override fun onIsPlayingChanged(isPlaying: Boolean) = playingChanged(isPlaying)

            // A pause or play while buffering changes nothing that onIsPlayingChanged sees: report it
            // here, paused coming from playWhenReady (Trakt's scrobble follows it).
            override fun onPlayWhenReadyChanged(
                playWhenReady: Boolean,
                reason: Int,
            ) {
                // Under another page nothing starts it again: not the remote's Play key through the media
                // session, not a picture-in-picture button.
                if (playWhenReady && covered) {
                    pause()
                    return
                }
                report()
            }

            override fun onRenderedFirstFrame() = renderedFirstFrame()

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK) seekPending = true
            }

            override fun onTracksChanged(tracks: Tracks) = refreshTracks(tracks)

            override fun onPlayerError(error: PlaybackException) = failed(error)

            override fun onVideoSizeChanged(videoSize: VideoSize) {
                if (videoSize.height >
                    0
                ) {
                    videoRatio = videoSize.width * videoSize.pixelWidthHeightRatio / videoSize.height
                }
            }

            override fun onPlaybackParametersChanged(playbackParameters: PlaybackParameters) {
                speed = playbackParameters.speed
            }
        }

    init {
        player.addListener(listener)
        ticker = scope.launch { tickLoop() }
        // A start kept going in the background ([StartService]) is let go once it is over: Ready, a failure or the end.
        scope.launch { snapshotFlow { starting }.collect { if (!it) StartService.release() } }
    }

    /** The core's playback view is open: follow it. */
    fun attach(playback: PlaybackView) {
        view = playback
        mark("playback open")
        scope.launch { snapshotFlow { playback.snapshot }.filterNotNull().collect(::snapshotChanged) }
        // The core going away takes the view with it (a restart subscribes again with ids that no longer
        // mean anything): say so instead of leaving a spinner or a player that nobody is listening to.
        scope.launch {
            snapshotFlow { playback.status }.collect { status ->
                if (status is ViewStatus.Failed && stage != Stage.Failed) {
                    player.stop()
                    player.clearMediaItems()
                    loadedUrl = null
                    fail(status.reason)
                }
            }
        }
    }

    fun showStatus(line: String?) {
        if (stage == Stage.Opening || stage == Stage.Buffering) status = line
    }

    /**
     * The version could not be opened (the core no longer lists it): "This version could not be opened", [reason]
     * under it, and Try again calls [again] (review 2026-10-20 r3 P3-9, as on Apple).
     */
    fun cannotOpen(
        reason: String,
        again: () -> Unit,
    ) {
        stage = Stage.Failed
        problem = CANNOT_OPEN
        openError = reason
        opener = again
        status = null
    }

    /** Try again on a version that could not be opened: it is opened again. */
    fun reopen() {
        val again = opener ?: return
        opener = null
        openError = null
        problem = null
        stage = Stage.Opening
        again()
    }

    /** Nothing can be played and the core has no attempts to show (no version, or the core did not answer). */
    fun fail(reason: String) {
        stage = Stage.Failed
        problem = reason
        status = null
    }

    fun mark(name: String) {
        marks.getOrPut(name) { SystemClock.elapsedRealtime() - openedAtMs }
    }

    // The core

    private fun snapshotChanged(s: PlaybackSnapshot) {
        if (released) return
        attempts = s.attempts
        next = s.next
        bingeGroup = s.bingeGroup
        segments = s.segments
        nextAtMs = s.nextAtMs
        trakt = s.trakt
        offer = s.offer
        retryable = s.phase == PlaybackPhase.Failed
        // A Try again the core took since the last snapshot, even when the play then failed again at once
        // and the snapshot is otherwise the one before (code review 2026-10-20 r2 P3-6).
        val retried = retriedSince(retriesSeen, s.retries)
        retriesSeen = s.retries
        start = s.start
        // While a source is being found its list is still to come: not "None found".
        subtitlesPending = s.subtitlesPending || s.phase == PlaybackPhase.Resolving
        snapshotSubs = s.subtitles
        val added = newSubtitleNotes(subtitleNotes, s.subtitleNotes)
        subtitleNotes = s.subtitleNotes
        // A note while one of the core's tracks is on is about it (only the picked file is fetched).
        if (pick?.external == true) added.lastOrNull()?.let { announce(subtitleNoteWords(it)) }
        // The picked file left the list (the core could not read it): it goes off quietly, as the note above
        // already said so. Not on a new source, whose list the load below carries the pick over to.
        if (s.url != null && s.url == loadedUrl) {
            pick?.url?.takeIf { url -> s.subtitles.none { it.url == url } }?.let { dropTrack(it, words = null) }
        }
        // The core's start subtitle, once, as if picked; nothing goes back to the core.
        startSubtitle(s.autoSubtitle, chose, pick != null, started)?.let { url ->
            s.subtitles.firstOrNull { it.url == url }?.let(::startWith)
        }
        when (s.phase) {
            PlaybackPhase.Ready -> {
                val url = s.url
                if (url != null && url != loadedUrl) load(s, url)
            }

            PlaybackPhase.Resolving -> {
                // The core starts this play over (Try again): the failure card goes.
                if (stage == Stage.Failed) {
                    stage = Stage.Opening
                    problem = null
                    loadingFill = 0f
                }
                s.start?.let { loadingFill = maxOf(loadingFill, it.progress) }
                // The core moved on to the ranking's next version by itself: say which.
                val trying = s.trying
                if (trying != null) {
                    status = "Trying the next version: ${trying.label}"
                    // The Versions button names the version being tried.
                    playingKey = trying.key
                } else if (retried) {
                    status = TRYING_AGAIN
                } else if (handlingFailure) {
                    val last = s.attempts.lastOrNull()
                    status =
                        last?.let { "Source failed (${it.via}: ${it.error}), trying the next one" }
                            ?: "Trying the next source"
                }
            }

            PlaybackPhase.Failed -> {
                player.stop()
                player.clearMediaItems()
                loadedUrl = null
                stage = Stage.Failed
                problem = failedHeadline(offered = s.offer != null)
                status = null
                if (retried) sayTriedAgain()
            }

            // A phase from a newer core: keep what plays until a known phase comes.
            PlaybackPhase.Unknown -> {
                Unit
            }
        }
        refreshTracks()
    }

    private fun load(
        s: PlaybackSnapshot,
        url: String,
    ) {
        val again = loadedUrl != null || handlingFailure
        val from = if (again) lastPositionMs else s.resumeMs ?: 0L
        loadedUrl = url
        playingKey = s.key
        playingAttempt = s.attempt
        durationMs = 0L
        skippedAtMs = null
        handlingFailure = false
        raised = false
        seekPending = false
        // A new source reports nothing until it has shown its own first frame.
        firstFrame = false
        picture = false
        fillBase = loadingFill
        pictureWait.reset()
        mark("url ready")
        player.setMediaItem(item(url), from.coerceAtLeast(0L))
        player.playWhenReady = false
        player.prepare()
        stage = Stage.Buffering
        // A new source lists its files at new addresses: the addon pick goes over to the same file, else the
        // best one in its language, else off.
        pick?.takeIf { it.external }?.let { carried ->
            val next = carriedSubtitle(s.subtitles.filterNot { it.url in dead }, carried.language, carried.url)
            when {
                next == null -> choose(TRACK_OFF)
                next.url != carried.url -> turnOn(next)
            }
        }
        if (again && !reopening) announce("Playing a different source")
        reopening = false
    }

    private fun item(url: String): MediaItem =
        MediaItem
            .Builder()
            .setUri(url)
            .setMediaMetadata(
                MediaMetadata
                    .Builder()
                    .setTitle(title)
                    .setSubtitle(episode)
                    .setArtworkUri(artwork?.toUri())
                    .build(),
            ).build()

    /** The card's "Trying again" for a moment, when the core took a Try again and the play failed again at once. */
    private fun sayTriedAgain() {
        tryingAgain = true
        tryingAgainJob?.cancel()
        tryingAgainJob =
            scope.launch {
                delay(TRYING_AGAIN_MS)
                tryingAgain = false
            }
    }

    /** A line for a few seconds (a source change), shown over the picture. */
    private fun announce(line: String) {
        notice = line
        noticeJob?.cancel()
        noticeJob =
            scope.launch {
                delay(NOTICE_MS)
                notice = null
            }
    }

    // The player

    private fun stateChanged(state: Int) {
        when (state) {
            Player.STATE_BUFFERING -> {
                // A stall or a seek over the picture: the loading screen fills again from the buffer.
                if (picture && stage != Stage.Buffering) {
                    loadingFill = 0f
                    fillBase = 0f
                }
                if (loadedUrl != null) stage = Stage.Buffering
                // Not a seek or a new source: the connection fell behind the file.
                if (firstFrame && player.playWhenReady && !seekPending) {
                    stalledAt = SystemClock.elapsedRealtime()
                }
            }

            Player.STATE_READY -> {
                mark("buffer ready")
                if (stalledAt != 0L) {
                    CoreLog.d("stall ${SystemClock.elapsedRealtime() - stalledAt} ms")
                    stalledAt = 0L
                }
                if (!raised) {
                    raised = true
                    if (wantPlay) player.playWhenReady = true
                }
                stage = if (player.isPlaying || player.playWhenReady) Stage.Playing else Stage.Paused
                if (seekPending) {
                    seekPending = false
                    report()
                }
            }

            Player.STATE_ENDED -> {
                report()
                stage = Stage.Ended
            }

            else -> {
                Unit
            }
        }
    }

    private fun playingChanged(isPlaying: Boolean) {
        if (isPlaying) {
            // Playing again after a pause: Trakt hears it now, not at the next report.
            if (stage == Stage.Paused) report()
            stage = Stage.Playing
        } else if (player.playbackState == Player.STATE_READY && raised) {
            stage = Stage.Paused
            report()
        }
    }

    private fun renderedFirstFrame() {
        firstFrame = true
        picture = true
        status = null
        if (firstFrameMs == null) {
            mark("first frame")
            firstFrameMs = marks["first frame"]
            CoreLog.d("first frame ${marks.entries.joinToString(", ") { "${it.key} ${it.value} ms" }}")
        }
    }

    private fun failed(error: PlaybackException) {
        // A late error from a source already given up on must not fail the next one.
        val playback = view
        if (handlingFailure || playback == null || released) return
        val cause = error.cause as? HttpDataSource.InvalidResponseCodeException
        // How the renderer that raised it supports the file: past the decoder, a decoder that did not start is decode.
        val support =
            (error as? ExoPlaybackException)
                ?.takeIf { it.type == ExoPlaybackException.TYPE_RENDERER }
                ?.rendererFormatSupport
        val failure = classifyFailure(error.errorCode, cause?.responseCode, support)
        CoreLog.w("playback error ${error.errorCodeName} (format support ${support ?: "none"}): ${failure.message}")
        failOver(playback, failure)
    }

    /** No picture came though the source had what one needs ([waitsForPicture]): a decode failure, in the same words. */
    private fun noPicture() {
        val playback = view
        if (handlingFailure || playback == null || released) return
        CoreLog.w("no picture $NO_PICTURE_MS ms after the source had what a first frame needs")
        failOver(playback, classifyFailure(PlaybackException.ERROR_CODE_DECODING_FAILED, null))
    }

    private fun failOver(
        playback: PlaybackView,
        failure: Failure,
    ) {
        lastPositionMs = player.currentPosition.coerceAtLeast(0L)
        handlingFailure = true
        player.stop()
        player.clearMediaItems()
        startOver()
        stage = Stage.Buffering
        status = "${failure.message}, trying the next source"
        playback.failOver(failure.message, playingAttempt, failure.kind.wire, failure.status)
    }

    /**
     * An addon file that did not load, or that the core could not read: off if it was on, not offered again in this
     * play, and [words] said for a few seconds when given. The video goes on.
     */
    private fun dropTrack(
        url: String,
        words: String?,
    ) {
        CoreLog.w("a subtitle file could not be read: dropped, the video goes on")
        dead += url
        fetched.remove(url)
        if (pick?.url == url) {
            pick = null
            hideAddon()
        }
        refreshTracks()
        words?.let(::announce)
    }

    private suspend fun tickLoop() {
        var sinceReport = 0L
        while (true) {
            delay(TICK_MS)
            if (loadedUrl == null || handlingFailure) continue
            positionMs = player.currentPosition.coerceAtLeast(0L)
            // Unknown while a source is being set again: keep the last length so the bar does not flicker.
            durationMs = player.duration.takeIf { it > 0 } ?: durationMs
            sendDuration()
            bufferedMs = player.bufferedPosition.coerceAtLeast(0L)
            if (stage == Stage.Buffering) {
                val threshold = if (picture) REBUFFER_MS else START_BUFFER_MS
                loadingFill = maxOf(loadingFill, bufferFill(fillBase, bufferedMs - positionMs, threshold.toLong()))
            }
            val waiting =
                waitsForPicture(
                    firstFrame,
                    player.currentTracks.containsType(C.TRACK_TYPE_VIDEO),
                    wantPlay,
                    player.playbackState == Player.STATE_READY,
                    bufferedMs - positionMs,
                )
            if (pictureWait.outOfTime(waiting, SystemClock.elapsedRealtime())) {
                noPicture()
                continue
            }
            if (player.isPlaying) {
                sinceReport += TICK_MS
                if (sinceReport >= REPORT_MS) {
                    sinceReport = 0
                    report()
                }
            } else {
                sinceReport = 0
            }
        }
    }

    /** The core looks skip segments up by the length of the version that plays: tell it once per version. */
    private fun sendDuration() {
        val key = playingKey ?: return
        val length = player.duration
        if (length <= 0 || durationSentFor == key) return
        durationSentFor = key
        CoreLog.d("duration $length ms sent for the playing version")
        view?.duration(key, length)
    }

    private fun report() {
        val playback = view ?: return
        if (loadedUrl == null || handlingFailure) return
        // The real position, also at the end: ExoPlayer can reach ENDED early on a truncated file, and
        // reporting the duration then would mark a part-watched film as finished.
        val ended = player.playbackState == Player.STATE_ENDED
        val suppressed = player.playbackSuppressionReason != Player.PLAYBACK_SUPPRESSION_REASON_NONE
        progressReport(
            firstFrame,
            player.currentPosition,
            player.duration,
            player.playWhenReady,
            ended,
            suppressed,
        )?.let {
            CoreLog.d("progress ${it.positionMs} of ${it.durationMs} ms${if (it.paused) ", paused" else ""}")
            playback.progress(it.positionMs, it.durationMs, it.paused)
        }
    }

    // What the viewer does

    /**
     * "Try again": the core plays the same versions again from its first pick; what failed may be the moment.
     * The card stays until the core says it starts over ([PlaybackPhase.Resolving]): a source that fails again
     * at once leaves the core's snapshot as it was, and the card must not go away for good. A press while the
     * core already starts over does nothing there.
     */
    fun retry() {
        if (!retryable || released) return
        view?.retryPlay()
    }

    fun togglePlay() {
        // At the end the player still has playWhenReady set, but the viewer's press means "again".
        if (stage == Stage.Ended) {
            play()
        } else if (stage == Stage.Playing || player.playWhenReady) {
            pause()
        } else {
            play()
        }
    }

    fun play() {
        wantPlay = true
        if (stage == Stage.Ended) player.seekTo(0L)
        player.playWhenReady = true
    }

    fun pause() {
        wantPlay = false
        player.playWhenReady = false
    }

    fun seekTo(ms: Long) {
        val end = player.duration.takeIf { it > 0 } ?: Long.MAX_VALUE
        player.seekTo(ms.coerceIn(0L, end))
        positionMs = player.currentPosition.coerceAtLeast(0L)
    }

    fun seekBy(deltaMs: Long) = seekTo(player.currentPosition + deltaMs)

    /**
     * The Skip button: seek to the end of the [segment]. Never automatic. Credits that run to the end of the
     * file lead into Up Next when there is a next episode, and otherwise to the end.
     */
    fun skip(segment: Segment) {
        if (!creditsToEnd(segment, durationMs)) {
            seekTo(segment.endMs)
        } else if (next != null) {
            skippedAtMs = segment.startMs
        } else {
            seekTo(durationMs - 50)
        }
    }

    fun changeSpeed(value: Float) = player.setPlaybackSpeed(value)

    fun selectAudio(id: String) {
        val (group, index) = find(C.TRACK_TYPE_AUDIO, id) ?: return
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, index))
                .build()
    }

    /**
     * The viewer's pick from the subtitle menu (or Off): it wins for the rest of the play, and a core track
     * or Off goes back to the core, which remembers it for the title (`subtitleChosen`). A track in the
     * file is the viewer's pick too, but the core does not know it. Nothing here sets the media item again.
     */
    fun selectText(id: String) {
        chose = true
        when {
            id == TRACK_OFF -> view?.subtitleChosen(null)
            id.startsWith(LATE) -> view?.subtitleChosen(id.removePrefix(LATE))
        }
        choose(id)
    }

    /**
     * The core's start subtitle, as if picked: once the file has opened, the file's own track in its language takes
     * its place ([fileTakesOver]), as it always fits the file.
     */
    private fun startWith(track: SubtitleTrack) {
        started = track.url
        val own = fileTakesOver(languageKey(track.language.ifEmpty { track.lang }), text)
        if (own != null) choose(own.id) else turnOn(track)
    }

    /** The file opened after the start subtitle came on: its own track in that language takes over ([startWith]). */
    private fun fileTakesOverStart() {
        val auto = pick?.takeIf { it.external && !chose && it.url != null && it.url == started } ?: return
        val track = snapshotSubs.firstOrNull { it.url == auto.url } ?: return
        fileTakesOver(languageKey(track.language.ifEmpty { track.lang }), text)?.let { choose(it.id) }
    }

    /**
     * Turns one of the core's tracks on, picked from the menu or the start subtitle: drawn over the picture, the
     * file's own tracks off under it.
     */
    private fun turnOn(track: SubtitleTrack) {
        pick = TextPick(track.lang, external = true, url = track.url)
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                .build()
        showAddon(track)
        refreshTracks()
    }

    /** Draws [track] from its file: at once when this play parsed it already, else once it is in. */
    private fun showAddon(track: SubtitleTrack) {
        fetching?.cancel()
        fetched[track.url]?.let {
            shown = it
            return
        }
        shown = null
        fetching =
            scope.launch {
                val cues =
                    try {
                        val bytes = fetch(track.url)
                        withContext(Dispatchers.Default) { parseAddonCues(bytes, subtitleMime(track.url)) }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        CoreLog.w("subtitle file ${track.source}: ${e.message}")
                        null
                    }
                if (cues == null) {
                    dropTrack(track.url, "Subtitles from ${track.source} did not load")
                    return@launch
                }
                fetched[track.url] = cues
                if (pick?.url == track.url) shown = cues
            }
    }

    private fun hideAddon() {
        fetching?.cancel()
        shown = null
    }

    /** An addon track is drawn over the picture: the player's own cues give way to [addonCues]. */
    val addonOn: Boolean get() = shown != null

    /**
     * The addon track's cues at [positionMs], moved by the track's delay (later when positive); none with no addon
     * track on. The picture's subtitles read it every frame while [addonOn].
     */
    fun addonCues(positionMs: Long = player.currentPosition): List<Cue> =
        shown?.at((positionMs - subtitleDelayMs) * 1000) ?: emptyList()

    private fun choose(id: String) {
        val params = player.trackSelectionParameters.buildUpon()
        when {
            id == TRACK_OFF -> {
                pick = null
                hideAddon()
                player.trackSelectionParameters = params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true).build()
            }

            id.startsWith(LATE) -> {
                val late = snapshotSubs.firstOrNull { it.url == id.removePrefix(LATE) } ?: return
                turnOn(late)
                return
            }

            else -> {
                val (group, index) = find(C.TRACK_TYPE_TEXT, id) ?: return
                val format = group.getTrackFormat(index)
                hideAddon()
                pick =
                    TextPick(
                        language = format.language.orEmpty(),
                        external = false,
                        group = group.mediaTrackGroup.id,
                        kind = format.codecs ?: format.sampleMimeType,
                    )
                player.trackSelectionParameters =
                    params
                        .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                        .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, index))
                        .build()
            }
        }
        refreshTracks()
    }

    /**
     * Leaving the screen (Home, another app, the picture-in-picture window closing): stop the sound. A start under way
     * goes on, paused (plans/player-polish.md 2c), and plays when the viewer is back.
     */
    fun onStop() {
        if (starting && wantPlay) playOnReturn = true
        pause()
    }

    /** Back on the screen: a start that went on while the app was away plays now, at its point. */
    fun onStart() {
        if (!playOnReturn) return
        playOnReturn = false
        play()
    }

    /**
     * The Timing row: the chosen track's subtitles show [ms] later (earlier when negative) for the rest of this play,
     * within [SUBTITLE_MAX_MS]. With no subtitle on there is no track to time, and nothing changes.
     */
    fun setSubtitleDelay(ms: Long) {
        val chosen = pick ?: return
        val next = ms.coerceIn(-SUBTITLE_MAX_MS, SUBTITLE_MAX_MS)
        delays[chosen.delayKey] = next
        applyDelay(next)
        // A core track's delay goes to the core once a run of presses settles, so its next play starts there.
        val url = chosen.url?.takeIf { chosen.external } ?: return
        unsentDelay = url to next
        delaySend?.cancel()
        delaySend =
            scope.launch {
                delay(DELAY_SETTLE_MS)
                sendDelay()
            }
    }

    /** The delay set on a core track and not yet told to the core: its address and ms. */
    private var unsentDelay: Pair<String, Long>? = null
    private var delaySend: Job? = null

    private fun sendDelay() {
        val (url, ms) = unsentDelay ?: return
        unsentDelay = null
        delaySend?.cancel()
        view?.subtitleDelay(url, ms)
    }

    /** Where a track's delay starts in this play: a core track's where the viewer last left it (`delayMs`). */
    private fun startDelay(pick: TextPick): Long =
        pick.url
            ?.let { url -> snapshotSubs.firstOrNull { it.url == url } }
            ?.delayMs
            ?.toLong() ?: 0L

    /** A track's identity for its delay: the addon file's address, or the file's own track group. */
    private val TextPick.delayKey: String get() = url ?: group ?: language

    /** The chosen track's delay is [ms], drawn at once: by the app for an addon track, by the text renderer else. */
    private fun applyDelay(ms: Long) {
        subtitleDelayMs = ms
        subtitleDelay.ms = ms
    }

    /** A subtitle is on (a track other than Off is chosen): the subtitle panel shows its Timing row. */
    val subtitleOn: Boolean get() = text.any { it.selected && it.id != TRACK_OFF }

    /**
     * Another page is over the player (Versions, an install link): it stays paused, whatever asks it to
     * play. The screen sets it as the player leaves and comes back to the screen.
     */
    var covered = false
        set(value) {
            field = value
            if (value) pause()
        }

    /**
     * The core's stream server took another port while the app was away: the loaded address is gone, so
     * the play is asked for again, and the new address loads where this one was.
     */
    fun serverMoved() {
        if (loadedUrl == null || released) return
        lastPositionMs = player.currentPosition.coerceAtLeast(0L)
        reopening = true
        startOver()
        view?.retry()
    }

    /** A source that failed or moved is opened again: the loading screen starts from nothing, not where the last got. */
    private fun startOver() {
        picture = false
        loadingFill = 0f
        fillBase = 0f
    }

    // The same source at a new address (the server moved): nothing to announce when it loads.
    private var reopening = false

    private fun find(
        type: Int,
        id: String,
    ): Pair<Tracks.Group, Int>? {
        val groups = player.currentTracks.groups
        groups.forEachIndexed { gi, group ->
            if (group.type != type) return@forEachIndexed
            for (ti in 0 until group.length) if (trackId(gi, ti, group) == id) return group to ti
        }
        return null
    }

    private fun trackId(
        groupIndex: Int,
        trackIndex: Int,
        group: Tracks.Group,
    ): String = "$groupIndex:$trackIndex"

    private fun refreshTracks(tracks: Tracks = player.currentTracks) {
        val audioRows = ArrayList<TrackChoice>()
        val fileRows = ArrayList<TrackChoice>()
        tracks.groups.forEachIndexed { gi, group ->
            for (ti in 0 until group.length) {
                if (!group.isTrackSupported(ti)) continue
                val format = group.getTrackFormat(ti)
                val id = trackId(gi, ti, group)
                val selected = group.isTrackSelected(ti)
                when (group.type) {
                    C.TRACK_TYPE_AUDIO -> {
                        audioRows += audioChoice(id, format, selected)
                    }

                    C.TRACK_TYPE_TEXT -> {
                        fileRows += textChoice(id, format, selected)
                    }
                }
            }
        }
        // The addon pick is on from the moment it is picked, while its file comes in.
        val on = pick?.takeIf { it.external }?.url
        val loaded = on?.let { mapOf(it to TrackChoice("$LATE$it", "", selected = true)) } ?: emptyMap()
        audio = audioRows
        text = subtitleRows(snapshotSubs, loaded, dead, fileRows, LATE)
        applyPick(tracks)
        fileTakesOverStart()
    }

    /**
     * When the tracks change (a failover, a reload, or the track list of this item filling in) the
     * viewer's subtitle is found again with [findPick]. Does nothing when it is already the one on.
     */
    private fun applyPick(tracks: Tracks) {
        // An addon pick is drawn over the picture: nothing in the player to find.
        val wanted = pick?.takeUnless { it.external } ?: return
        val refs = ArrayList<Pair<Tracks.Group, Int>>()
        val found = ArrayList<TextCandidate>()
        tracks.groups.forEach { group ->
            if (group.type != C.TRACK_TYPE_TEXT) return@forEach
            for (ti in 0 until group.length) {
                if (!group.isTrackSupported(ti)) continue
                val format = group.getTrackFormat(ti)
                refs += group to ti
                found +=
                    TextCandidate(
                        external = false,
                        url = null,
                        group = group.mediaTrackGroup.id,
                        kind = format.codecs ?: format.sampleMimeType,
                        language = format.language,
                    )
            }
        }
        val (group, index) = findPick(wanted, found)?.let(refs::get) ?: return
        if (group.isTrackSelected(index)) return
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, index))
                .build()
    }

    /** Reports where the player is and lets go of it. */
    fun release() {
        if (released) return
        report()
        sendDelay()
        released = true
        StartService.release()
        ticker?.cancel()
        noticeJob?.cancel()
        fetching?.cancel()
        tryingAgainJob?.cancel()
        player.removeListener(listener)
        view?.close()
        view = null
    }

    private companion object {
        const val TICK_MS = 500L
        const val REPORT_MS = 5_000L
        const val NOTICE_MS = 4_000L
        const val TRYING_AGAIN_MS = 1_500L
        const val LATE = "late:"

        /** How long after the last delay press a core track's delay goes to the core. */
        const val DELAY_SETTLE_MS = 700L
    }
}
