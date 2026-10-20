package app.marquee.player

import android.os.SystemClock
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
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlaybackException
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Attempt
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.OtherVersion
import app.marquee.core.model.PlaybackPhase
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.Segment
import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleTrack
import app.marquee.core.model.TraktPlayback
import app.marquee.core.views.PlaybackView
import app.marquee.core.views.ViewStatus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch

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
 * - Subtitles the core finds after the media item was set cannot be added to it, so choosing one
 *   sets the item again at the current position with the longer list.
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
    var segments: List<Segment> by mutableStateOf(emptyList())
        private set

    /** The position the core says the Up Next card is due (the credits start); null means the last seconds. */
    var nextAtMs: Long? by mutableStateOf(null)
        private set

    /** Trakt's side of this play, for the player's mark; null until the core says. */
    var trakt: TraktPlayback? by mutableStateOf(null)
        private set

    /** Where Skip credits was pressed: Up Next shows at once from there, until the viewer seeks back before it. */
    var skippedAtMs: Long? by mutableStateOf(null)
        private set

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

    private var view: PlaybackView? = null
    private var loadedUrl: String? = null
    private var loadedSubs: List<SubtitleTrack> = emptyList()
    private var snapshotSubs: List<SubtitleTrack> = emptyList()
    private var firstFrame = false
    private var raised = false
    private var seekPending = false

    /** When the buffer ran dry during play, 0 when it did not: the race counts stalls (plans/m13-stream-choice.md 6.4). */
    private var stalledAt = 0L
    private var handlingFailure = false
    private var lastPositionMs = 0L
    private var pick: TextPick? = null

    /** The viewer picked a subtitle or Off in this play: the core's start subtitle no longer turns on. */
    private var chose = false

    /** The start subtitle already turned on, so a repeat of it does nothing. */
    private var started: String? = null

    /** Addon subtitle files that did not load in this play: not side-loaded or offered again. */
    private val dead = HashSet<String>()
    private var released = false
    private var durationSentFor: String? = null
    private var reloadedAt = 0L
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
            ) = report()

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
        // While a source is being found its list is still to come: not "None found".
        subtitlesPending = s.subtitlesPending || s.phase == PlaybackPhase.Resolving
        snapshotSubs = s.subtitles
        val added = newSubtitleNotes(subtitleNotes, s.subtitleNotes)
        subtitleNotes = s.subtitleNotes
        // A note while one of the core's tracks is on is about it (only the picked file is fetched).
        if (pick?.external == true) added.lastOrNull()?.let { announce(subtitleNoteWords(it)) }
        // A side-loaded file that left the list (the core could not read it) leaves this item too, quietly:
        // the note above already said so. Not on a new source, whose files the load below takes.
        if (s.url != null && s.url == loadedUrl) {
            val gone = loadedSubs.filter { sub -> s.subtitles.none { it.url == sub.url } }
            if (gone.isNotEmpty()) dropSubtitles(gone, words = null)
        }
        // The core's start subtitle, once, as if picked; nothing goes back to the core.
        startSubtitle(s.autoSubtitle, chose, pick != null, started)?.let { url ->
            s.subtitles.firstOrNull { it.url == url }?.let { track ->
                started = url
                turnOn(track)
            }
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
                }
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
        val carried = pick?.takeIf { it.external }
        loadedSubs = upfrontSubtitles(s.subtitles.filterNot { it.url in dead }, carried?.language, carried?.url)
        playingKey = s.key
        playingAttempt = s.attempt
        durationMs = 0L
        skippedAtMs = null
        handlingFailure = false
        raised = false
        seekPending = false
        // A new source reports nothing until it has shown its own first frame.
        firstFrame = false
        pictureWait.reset()
        mark("url ready")
        player.setMediaItem(item(url, loadedSubs), from.coerceAtLeast(0L))
        player.playWhenReady = false
        player.prepare()
        stage = Stage.Buffering
        if (again) announce("Playing a different source")
    }

    private fun item(
        url: String,
        subs: List<SubtitleTrack>,
    ): MediaItem =
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
            ).setSubtitleConfigurations(
                subs.mapIndexed { i, t ->
                    MediaItem.SubtitleConfiguration
                        .Builder(t.url.toUri())
                        .setId("$EXTERNAL$i")
                        .setMimeType(subtitleMime(t.url))
                        .setLanguage(t.lang)
                        .setLabel(t.label)
                        .build()
                },
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
                if (loadedUrl != null) stage = Stage.Buffering
                // Not a seek, a subtitle reload or a new source: the connection fell behind the file.
                if (firstFrame && player.playWhenReady && !seekPending && reloadedAt == 0L) {
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
                if (reloadedAt != 0L) {
                    CoreLog.d("subtitle reload ${SystemClock.elapsedRealtime() - reloadedAt} ms")
                    reloadedAt = 0L
                }
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
        // A subtitle file that did not load costs that track, never the video.
        failedSubtitle(error, loadedSubs)?.let {
            dropSubtitles(listOf(it), "Subtitles from ${it.source} did not load")
            return
        }
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
        stage = Stage.Buffering
        status = "${failure.message}, trying the next source"
        playback.failOver(failure.message, playingAttempt, failure.kind.wire, failure.status)
    }

    /**
     * Sets the item again without [subs] at the same position; if one of them was on, subtitles go off. Says
     * [words] for a few seconds when given. The files are not side-loaded or offered again in this play.
     */
    private fun dropSubtitles(
        subs: List<SubtitleTrack>,
        words: String?,
    ) {
        CoreLog.w("${subs.size} subtitle file(s) could not be read: dropped, the video goes on")
        val urls = subs.map { it.url }.toSet()
        dead += urls
        if (pick?.url in urls) {
            pick = null
            player.trackSelectionParameters =
                player.trackSelectionParameters
                    .buildUpon()
                    .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                    .build()
        }
        val url = loadedUrl ?: return
        val at = player.currentPosition.coerceAtLeast(0L)
        reloadedAt = SystemClock.elapsedRealtime()
        loadedSubs = loadedSubs.filter { it.url !in urls }
        player.setMediaItem(item(url, loadedSubs), at)
        player.prepare()
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
            if (player.isPlaying && ranPastEnd(positionMs, player.duration, loadedSubs.isNotEmpty())) endAtVideoEnd()
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

    /** The video ended but a longer subtitle would keep the player going on black ([ranPastEnd]): the play is over. */
    private fun endAtVideoEnd() {
        CoreLog.d("a subtitle runs past the video's end at ${player.duration} ms: the play ends there")
        // Heard as a pause first (and reported so), then it is the end.
        player.playWhenReady = false
        stage = Stage.Ended
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
     * file is the viewer's pick too, but the core does not know it.
     */
    fun selectText(id: String) {
        chose = true
        val url =
            when {
                id == TRACK_OFF -> {
                    null
                }

                id.startsWith(LATE) -> {
                    id.removePrefix(LATE)
                }

                id.startsWith(EXTERNAL) -> {
                    find(C.TRACK_TYPE_TEXT, id)?.let { (g, i) ->
                        externalUrl(g.getTrackFormat(i).id)
                    }
                }

                else -> {
                    return choose(id)
                }
            }
        if (id != TRACK_OFF && url == null) return choose(id)
        view?.subtitleChosen(url)
        choose(id)
    }

    /** Turns one of the core's tracks on: picked from the menu, or the start subtitle. */
    private fun turnOn(track: SubtitleTrack) {
        pick = TextPick(track.lang, external = true, url = track.url)
        // Before a source is loaded, the load takes the picked file with it.
        if (loadedUrl == null) return
        if (loadedSubs.any { it.url == track.url }) {
            applyPick(player.currentTracks)
            refreshTracks()
            return
        }
        // The override of an earlier pick would still match after the reload and keep the old track on.
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .clearOverridesOfType(C.TRACK_TYPE_TEXT)
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                .build()
        reloadWith(loadedSubs + track)
        refreshTracks()
    }

    private fun choose(id: String) {
        val params = player.trackSelectionParameters.buildUpon()
        when {
            id == TRACK_OFF -> {
                pick = null
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
                pick =
                    TextPick(
                        language = format.language.orEmpty(),
                        external = id.startsWith(EXTERNAL),
                        url = id.takeIf { it.startsWith(EXTERNAL) }?.let { externalUrl(format.id) },
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
     * Sets the item again at the same position with a longer subtitle list; a short hitch. After
     * the end the position is the end, which would play nothing, so it starts over paused instead.
     */
    private fun reloadWith(subs: List<SubtitleTrack>) {
        val url = loadedUrl ?: return
        // A source that was given up on or a player that was let go is not set again (the next
        // `ready` carries the whole list anyway).
        if (handlingFailure || released) return
        val ended = stage == Stage.Ended
        val at = if (ended) 0L else player.currentPosition.coerceAtLeast(0L)
        reloadedAt = SystemClock.elapsedRealtime()
        loadedSubs = subs
        player.setMediaItem(item(url, subs), at)
        if (ended) pause()
        player.prepare()
    }

    /** Leaving the screen (Home, another app, the picture-in-picture window closing): stop the sound. */
    fun onStop() = pause()

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
    ): String {
        val external = externalIndex(group.getTrackFormat(trackIndex).id)
        return if (external != null) "$EXTERNAL$external" else "$groupIndex:$trackIndex"
    }

    private fun refreshTracks(tracks: Tracks = player.currentTracks) {
        val audioRows = ArrayList<TrackChoice>()
        val fileRows = ArrayList<TrackChoice>()
        val loaded = HashMap<String, TrackChoice>()
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
                        // A side-loaded file is one of the core's tracks: listed with them, by its address.
                        val url = externalUrl(format.id)
                        if (url != null) {
                            loaded[url] = TrackChoice(id, "", selected = selected)
                        } else {
                            fileRows += textChoice(id, format, selected)
                        }
                    }
                }
            }
        }
        // The snapshot's order, then a loaded file the snapshot no longer lists (it leaves with the next reload).
        val core = snapshotSubs + loadedSubs.filter { sub -> snapshotSubs.none { it.url == sub.url } }
        // Left out: files that did not load, and side-loaded ones the player has not listed yet.
        val hidden = dead + loadedSubs.map { it.url }.filter { it !in loaded }
        audio = audioRows
        text = subtitleRows(core, loaded, hidden, fileRows, LATE)
        applyPick(tracks)
    }

    /** The address of the addon subtitle file behind a loaded track's `ext:N` id, or null. */
    private fun externalUrl(formatId: String?): String? = externalIndex(formatId)?.let { loadedSubs.getOrNull(it)?.url }

    /**
     * When the tracks change (a failover, a reload, or the track list of this item filling in) the
     * viewer's subtitle is found again with [findPick]. Does nothing when it is already the one on.
     */
    private fun applyPick(tracks: Tracks) {
        val wanted = pick ?: return
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
                        external = externalIndex(format.id) != null,
                        url = externalUrl(format.id),
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
        released = true
        ticker?.cancel()
        noticeJob?.cancel()
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
    }
}
