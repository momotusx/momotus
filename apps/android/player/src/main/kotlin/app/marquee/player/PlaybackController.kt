package app.marquee.player

import android.net.Uri
import android.os.SystemClock
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.common.VideoSize
import androidx.media3.common.util.Util
import androidx.media3.datasource.HttpDataSource
import app.marquee.core.client.CoreLog
import app.marquee.core.model.Attempt
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.PlaybackPhase
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.SubtitleTrack
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
    private val host: PlayerHost,
    private val scope: CoroutineScope,
    val title: String,
    /** "S1:E2", or null for a movie. */
    val episode: String?,
    private val artwork: String?,
    private val openedAtMs: Long,
) {
    enum class Stage { Opening, Buffering, Playing, Paused, Ended, Failed }

    val player: Player get() = host.player

    var stage by mutableStateOf(Stage.Opening)
        private set

    /** What is going on while nothing plays yet or the source changes ("Starting 1080p from Local Test"). */
    var status: String? by mutableStateOf(null)
        private set

    /** Why nothing plays, when [stage] is [Stage.Failed]. */
    var problem: String? by mutableStateOf(null)
        private set

    var attempts: List<Attempt> by mutableStateOf(emptyList())
        private set

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

    var next: NextEpisode? by mutableStateOf(null)
        private set

    /** Pass to Versions of the next episode so the same release line is picked. */
    var bingeGroup: String? = null
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

    private var view: PlaybackView? = null
    private var loadedUrl: String? = null
    private var loadedSubs: List<SubtitleTrack> = emptyList()
    private var snapshotSubs: List<SubtitleTrack> = emptyList()
    private var firstFrame = false
    private var raised = false
    private var seekPending = false
    private var handlingFailure = false
    private var lastPositionMs = 0L
    private var pick: TextPick? = null
    private var released = false
    private var reloadedAt = 0L
    private var ticker: Job? = null
    private var noticeJob: Job? = null
    private val marks = LinkedHashMap<String, Long>()

    /**
     * A subtitle the viewer chose. After the media item changes it is found again by its address
     * (an add-on's file), else by language, so the right track is on even when several share a language.
     */
    private data class TextPick(
        val language: String,
        val external: Boolean,
        val url: String? = null,
    )

    private val listener =
        object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) = stateChanged(playbackState)

            override fun onIsPlayingChanged(isPlaying: Boolean) = playingChanged(isPlaying)

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
        host.player.addListener(listener)
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
        subtitlesPending = s.subtitlesPending
        snapshotSubs = s.subtitles
        when (s.phase) {
            PlaybackPhase.Ready -> {
                val url = s.url
                if (url != null && url != loadedUrl) load(s, url)
            }

            PlaybackPhase.Resolving -> {
                if (handlingFailure) {
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
                problem = "None of the sources could be played"
                status = null
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
        loadedSubs = s.subtitles
        playingKey = s.key
        durationMs = 0L
        handlingFailure = false
        raised = false
        seekPending = false
        // A new source reports nothing until it has shown its own first frame.
        firstFrame = false
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
                    .setArtworkUri(artwork?.let(Uri::parse))
                    .build(),
            ).setSubtitleConfigurations(
                subs.mapIndexed { i, t ->
                    MediaItem.SubtitleConfiguration
                        .Builder(Uri.parse(t.url))
                        .setId("$EXTERNAL$i")
                        .setMimeType(subtitleMime(t.url))
                        .setLanguage(t.lang)
                        .setLabel(t.label)
                        .build()
                },
            ).build()

    /** A line for a few seconds (a source change), shown over the picture. */
    private fun announce(line: String) {
        status = line
        noticeJob?.cancel()
        noticeJob =
            scope.launch {
                delay(NOTICE_MS)
                status = null
            }
    }

    // The player

    private fun stateChanged(state: Int) {
        when (state) {
            Player.STATE_BUFFERING -> {
                if (loadedUrl != null) stage = Stage.Buffering
            }

            Player.STATE_READY -> {
                mark("buffer ready")
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
        val cause = error.cause as? HttpDataSource.InvalidResponseCodeException
        val failure = classifyFailure(error.errorCode, cause?.responseCode)
        CoreLog.w("playback error ${error.errorCodeName}: ${failure.message}")
        lastPositionMs = player.currentPosition.coerceAtLeast(0L)
        handlingFailure = true
        player.stop()
        player.clearMediaItems()
        stage = Stage.Buffering
        status = "${failure.message}, trying the next source"
        playback.failOver(failure.message)
    }

    private suspend fun tickLoop() {
        var sinceReport = 0L
        while (true) {
            delay(TICK_MS)
            if (loadedUrl == null || handlingFailure) continue
            positionMs = player.currentPosition.coerceAtLeast(0L)
            // Unknown while a source is being set again: keep the last length so the bar does not flicker.
            durationMs = player.duration.takeIf { it > 0 } ?: durationMs
            bufferedMs = player.bufferedPosition.coerceAtLeast(0L)
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

    private fun report() {
        val playback = view ?: return
        if (loadedUrl == null || handlingFailure) return
        // The real position, also at the end: ExoPlayer can reach ENDED early on a truncated file, and
        // reporting the duration then would mark a part-watched film as finished.
        progressReport(firstFrame, player.currentPosition, player.duration)?.let {
            CoreLog.d("progress ${it.positionMs} of ${it.durationMs} ms")
            playback.progress(it.positionMs, it.durationMs)
        }
    }

    // What the viewer does

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

    fun changeSpeed(value: Float) = player.setPlaybackSpeed(value)

    fun selectAudio(id: String) {
        val (group, index) = find(C.TRACK_TYPE_AUDIO, id) ?: return
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, index))
                .build()
    }

    fun selectText(id: String) {
        val params = player.trackSelectionParameters.buildUpon()
        when {
            id == TRACK_OFF -> {
                pick = null
                player.trackSelectionParameters = params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true).build()
            }

            id.startsWith(LATE) -> {
                val late = snapshotSubs.firstOrNull { it.url == id.removePrefix(LATE) } ?: return
                pick = TextPick(late.lang, external = true, url = late.url)
                // The override of an earlier pick would still match after the reload and keep the old track on.
                player.trackSelectionParameters =
                    params
                        .clearOverridesOfType(
                            C.TRACK_TYPE_TEXT,
                        ).setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                        .build()
                reloadWith(loadedSubs + late)
            }

            else -> {
                val (group, index) = find(C.TRACK_TYPE_TEXT, id) ?: return
                val format = group.getTrackFormat(index)
                pick =
                    TextPick(
                        language = format.language.orEmpty(),
                        external = id.startsWith(EXTERNAL),
                        url = id.takeIf { it.startsWith(EXTERNAL) }?.let { externalUrl(format.id) },
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
        val formatId = group.getTrackFormat(trackIndex).id
        return if (formatId != null && formatId.startsWith(EXTERNAL)) formatId else "$groupIndex:$trackIndex"
    }

    private fun refreshTracks(tracks: Tracks = player.currentTracks) {
        val audioRows = ArrayList<TrackChoice>()
        val textRows = ArrayList<TrackChoice>()
        val loaded = HashSet<String>()
        var textSelected = false
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
                        textRows += textChoice(id, format, selected).let { row -> externalDetail(row, format.id) }
                        textSelected = textSelected || selected
                    }
                }
            }
        }
        loadedSubs.forEach { loaded += it.url }
        val late =
            snapshotSubs.filter { it.url !in loaded }.map {
                val name = languageName(it.lang) ?: it.label ?: "Subtitles"
                TrackChoice(
                    "$LATE${it.url}",
                    name,
                    listOfNotNull(
                        it.label?.takeIf { l ->
                            l != name
                        },
                        it.source,
                        "loads when chosen",
                    ).joinToString(", "),
                )
            }
        audio = audioRows
        text = listOf(TrackChoice(TRACK_OFF, "Off", selected = !textSelected)) + textRows + late
        applyPick(tracks)
    }

    /** An external row says whose it is (the add-on) instead of a codec the viewer cannot act on. */
    private fun externalDetail(
        row: TrackChoice,
        formatId: String?,
    ): TrackChoice {
        val index = formatId?.takeIf { it.startsWith(EXTERNAL) }?.removePrefix(EXTERNAL)?.toIntOrNull() ?: return row
        val source = loadedSubs.getOrNull(index) ?: return row
        val detail =
            listOfNotNull(
                row.detail,
                source.source,
                "forced".takeIf {
                    source.forced &&
                        row.detail?.contains("forced") != true
                },
            ).joinToString(", ")
        return row.copy(detail = detail)
    }

    /** The address of the add-on subtitle file behind a loaded track's `ext:N` id, or null. */
    private fun externalUrl(formatId: String?): String? =
        formatId
            ?.takeIf { it.startsWith(EXTERNAL) }
            ?.removePrefix(EXTERNAL)
            ?.toIntOrNull()
            ?.let { loadedSubs.getOrNull(it)?.url }

    /** One text track of the current media item. */
    private class TextTrack(
        val group: Tracks.Group,
        val index: Int,
        val external: Boolean,
        val url: String?,
        val language: String?,
        val selected: Boolean,
    )

    /**
     * After a new media item (a failover or a reload) the viewer's subtitle is found again: first by
     * the address of the add-on file they chose, else by language (an add-on's file before an
     * embedded track when that is what they had). Does nothing when it is already the one on.
     */
    private fun applyPick(tracks: Tracks) {
        val wanted = pick ?: return
        val found = ArrayList<TextTrack>()
        tracks.groups.forEach { group ->
            if (group.type != C.TRACK_TYPE_TEXT) return@forEach
            for (ti in 0 until group.length) {
                if (!group.isTrackSupported(ti)) continue
                val format = group.getTrackFormat(ti)
                found +=
                    TextTrack(
                        group,
                        ti,
                        external = format.id?.startsWith(EXTERNAL) == true,
                        url = externalUrl(format.id),
                        language = format.language,
                        selected = group.isTrackSelected(ti),
                    )
            }
        }
        val same = { t: TextTrack ->
            wanted.language.isNotEmpty() &&
                t.language?.let { Util.normalizeLanguageCode(it) == Util.normalizeLanguageCode(wanted.language) } ==
                true
        }
        val target =
            found.firstOrNull { wanted.url != null && it.url == wanted.url }
                ?: found.firstOrNull { same(it) && it.external == wanted.external }
                ?: found.firstOrNull(same)
                ?: return
        if (target.selected) return
        player.trackSelectionParameters =
            player.trackSelectionParameters
                .buildUpon()
                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                .setOverrideForType(TrackSelectionOverride(target.group.mediaTrackGroup, target.index))
                .build()
    }

    /** Reports where the player is and lets go of it. */
    fun release() {
        if (released) return
        report()
        released = true
        ticker?.cancel()
        noticeJob?.cancel()
        host.player.removeListener(listener)
        view?.close()
        view = null
    }

    private companion object {
        const val TICK_MS = 500L
        const val REPORT_MS = 5_000L
        const val NOTICE_MS = 4_000L
        const val EXTERNAL = "ext:"
        const val LATE = "late:"
    }
}
