package app.marquee.player

import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.TrackGroup
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.common.Tracks
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlaybackException
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreLog
import app.marquee.core.model.OtherVersion
import app.marquee.core.views.PlaybackView
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue
import kotlin.test.fail

/**
 * A subtitle file that does not load (the addon answers 502 or times out) reaches the player as an
 * error of the whole item. It must cost that track, never the video: no failover, the same stream again.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class SubtitleFailureTest {
    private val video = "http://127.0.0.1:7780/video.mp4"
    private val subtitle = "http://127.0.0.1:7781/subsense/tur.srt"

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val sent = CopyOnWriteArrayList<JsonObject>()

    // The controller's work runs on the main looper, as on a device.
    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)

    private val other = "http://127.0.0.1:7781/opensubtitles/tur.srt"

    /** The core's playback snapshot with these subtitle files and notes, and [extra] fields ending in a comma. */
    private fun snapshot(
        subtitles: List<String> = listOf(subtitle),
        notes: String = "[]",
        phase: String = "ready",
        extra: String = "",
        url: String = video,
        pending: Boolean = false,
    ) = """{"phase":"$phase","url":"$url","key":"k1","attempt":1,"manifest":false,"subtitlesPending":$pending,""" +
        """"trakt":"off","subtitleNotes":$notes,$extra"subtitles":[""" +
        subtitles.joinToString(",") {
            """{"url":"$it","lang":"tur","source":"${if (it == subtitle) "SubSense" else "OpenSubtitles v3"}",""" +
                """"forced":false,"hearingImpaired":false}"""
        } + "]}"

    private val snapshot = snapshot()

    /** The core's view and a controller on a fake player, the stream loaded. */
    private fun open(first: String = snapshot): Pair<FakePlayer, PlaybackController> {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$first}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val player = FakePlayer()
        val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
        controller.attach(PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() })
        until("the stream loads") { player.items.size == 1 }
        return player to controller
    }

    /** A later snapshot of the core's view. */
    private fun push(next: String) = client.onMessage("""{"sub":7,"updates":[$next]}""")

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    /**
     * Lets the main looper, the core's state thread and Compose's snapshot changes catch up until [check], for up
     * to 10 s of real time: a count of rounds ran out in 2.5 s on a loaded machine (1 run in 13).
     */
    private fun until(
        what: String,
        check: () -> Boolean,
    ) {
        val end = System.nanoTime() + 10_000_000_000L
        while (System.nanoTime() < end) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            if (check()) return
            Thread.sleep(5)
        }
        fail(what)
    }

    /** Lets the main looper run for [ms] of real time: the controller's tick (every 500 ms) runs on real time. */
    private fun runFor(ms: Long) {
        val end = System.nanoTime() + ms * 1_000_000
        while (System.nanoTime() < end) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(5)
        }
    }

    /** One H.264 video track, as Media3 lists a file's video. */
    private val video264 =
        Tracks(
            listOf(
                Tracks.Group(
                    TrackGroup(Format.Builder().setSampleMimeType(MimeTypes.VIDEO_H264).build()),
                    false,
                    intArrayOf(C.FORMAT_HANDLED),
                    booleanArrayOf(true),
                ),
            ),
        )

    private fun commands(type: String) =
        sent.filter {
            it["command"]
                ?.jsonObject
                ?.get("type")
                ?.jsonPrimitive
                ?.content == type
        }

    private fun failOvers() = commands("failOver")

    /** What each `subtitleChosen` sent: a track's address, or null for Off. */
    private fun chosen() =
        commands("subtitleChosen").map {
            it["command"]!!.jsonObject["url"]?.jsonPrimitive?.contentOrNull
        }

    private fun httpError(
        url: String,
        status: Int,
    ) = ExoPlaybackException.createForSource(
        HttpDataSource.InvalidResponseCodeException(
            status,
            "Bad Gateway",
            null,
            emptyMap(),
            DataSpec(Uri.parse(url)),
            ByteArray(0),
        ),
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS,
    )

    @Test
    fun `a subtitle that does not load drops that track and the same stream plays on`() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$snapshot}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val player = FakePlayer()
        val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
        val view = PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() }
        controller.attach(view)
        until("the stream loads") { player.items.size == 1 }
        assertEquals(video, player.items.last().uri)

        // The viewer picks the Turkish file: the item is set again with it side-loaded.
        controller.selectText("late:$subtitle")
        until("the subtitle is side-loaded") { player.items.size == 2 }
        assertEquals(listOf(subtitle), player.items.last().subtitles)

        // The addon answers 502 for the file: Media3 fails the whole item.
        player.fail(httpError(subtitle, 502))
        until("the item is set again without the file") { player.items.size == 3 }
        assertEquals(video, player.items.last().uri)
        assertEquals(emptyList(), player.items.last().subtitles)
        assertEquals(0, player.stops, "the video was stopped")
        assertTrue(failOvers().isEmpty(), "a subtitle failed the video over")
        assertNotEquals(PlaybackController.Stage.Failed, controller.stage)
        assertEquals("Subtitles from SubSense did not load", controller.notice)
        assertEquals(null, controller.status, "the note came with the waiting mark")

        // The video's own failure still fails over.
        player.fail(httpError(video, 404))
        until("the video fails over") { failOvers().isNotEmpty() }
        controller.release()
    }

    @Test
    fun `a note while one of the core's tracks is on says so for a few seconds, and nothing else changes`() {
        val (player, controller) = open()
        controller.selectText("late:$subtitle")
        until("the subtitle is side-loaded") { player.items.size == 2 }
        push(snapshot(notes = """[{"source":"SubSense","problem":"unreachable"}]"""))
        until(
            "the note is said",
        ) { controller.notice == "SubSense did not answer. Try its subtitles again in a moment." }
        assertEquals(null, controller.status, "the note came with the waiting mark")
        assertEquals(2, player.items.size, "the item was set again")
        assertEquals("SubSense did not answer. Try its subtitles again in a moment.", controller.subtitlePanelNote)
        assertTrue(failOvers().isEmpty())
        // After its few seconds (four, timed on the real clock here) the line goes; the panel keeps the note.
        Thread.sleep(4_200)
        until("the line goes after a few seconds") { controller.notice == null }
        assertEquals("SubSense did not answer. Try its subtitles again in a moment.", controller.subtitlePanelNote)
        controller.release()
    }

    @Test
    fun `a note while no core track is on is listed but not said`() {
        val (_, controller) = open()
        push(snapshot(notes = """[{"source":"SubSense","problem":"blocked"}]"""))
        until("the note is listed") { controller.subtitlePanelNote != null }
        assertEquals(null, controller.notice)
        assertEquals(null, controller.status)
        controller.release()
    }

    @Test
    fun `a track the core could not read leaves the list, subtitles go off, the same stream plays on`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        controller.selectText("late:$subtitle")
        until("the subtitle is side-loaded") { player.items.size == 2 }
        // The core answered empty WebVTT and took the file out of the list, with a note.
        push(snapshot(subtitles = listOf(other), notes = """[{"source":"SubSense","problem":"trackFailed"}]"""))
        until("the item is set again without the file") { player.items.size == 3 }
        // The item set again shows its first frame at once: the note stays its few seconds.
        player.renderFirstFrame()
        until("the first frame is heard") { true }
        assertEquals(video, player.items.last().uri)
        assertEquals(emptyList(), player.items.last().subtitles)
        assertTrue(
            player.current.trackSelectionParameters.disabledTrackTypes
                .contains(C.TRACK_TYPE_TEXT),
            "subtitles stayed on",
        )
        // One message, the note's: no "did not load" on top.
        assertEquals("A subtitle file from SubSense could not be read. Pick another one.", controller.notice)
        assertEquals(null, controller.status, "the note came with the waiting mark")
        assertEquals(0, player.stops)
        assertTrue(failOvers().isEmpty())
        assertTrue(controller.text.none { it.id == "late:$subtitle" }, "the file is still offered")
        controller.release()
    }

    @Test
    fun `a new source sets its own item once, with no drop on top`() {
        val (player, controller) = open()
        controller.selectText("late:$subtitle")
        until("the subtitle is side-loaded") { player.items.size == 2 }
        // A failover: a new loopback URL, and the subtitle files under new addresses.
        val failedOver =
            snapshot(subtitles = listOf(other))
                .replace(video, "http://127.0.0.1:7780/second.mp4")
                .replace("\"attempt\":1", "\"attempt\":2")
        push(failedOver)
        until("the new source loads") { player.items.lastOrNull()?.uri == "http://127.0.0.1:7780/second.mp4" }
        repeat(20) { until("the looper idles") { true } }
        assertEquals(3, player.items.size, "the old item was set again before the new one")
        controller.release()
    }

    @Test
    fun `a buffer that runs dry during play is logged as a stall, a subtitle reload is not`() {
        val lines = CopyOnWriteArrayList<String>()
        CoreLog.sink = { _, message, _ -> lines += message }
        try {
            val (player, controller) = open()
            player.state(Player.STATE_READY)
            until("it plays") { player.playWhenReady }
            // Before the first frame the wait is the start, not a stall.
            player.state(Player.STATE_BUFFERING)
            player.state(Player.STATE_READY)
            player.renderFirstFrame()
            until("the first frame is heard") { true }
            // The connection falls behind for 1.2 s.
            player.state(Player.STATE_BUFFERING)
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(1_200))
            player.state(Player.STATE_READY)
            until("the stall is logged") { lines.any { it.startsWith("stall ") } }
            assertEquals(listOf("stall 1200 ms"), lines.filter { it.startsWith("stall ") })
            // A side-loaded file that does not load sets the item again: that wait is no stall.
            controller.selectText("late:$subtitle")
            until("the subtitle is side-loaded") { player.items.size == 2 }
            player.state(Player.STATE_READY)
            player.fail(httpError(subtitle, 502))
            until("the item is set again without the file") { player.items.size == 3 }
            player.state(Player.STATE_READY)
            until("the reload is logged") { lines.any { it.startsWith("subtitle reload") } }
            assertEquals(1, lines.count { it.startsWith("stall ") })
            controller.release()
        } finally {
            CoreLog.sink = { _, _, _ -> }
        }
    }

    @Test
    fun `the core's start subtitle turns on once by itself, and nothing goes back`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        push(snapshot(subtitles = listOf(subtitle, other), extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the start subtitle is side-loaded") { player.items.size == 2 }
        assertEquals(listOf(subtitle), player.items.last().subtitles)
        // Once: the same snapshot again changes nothing (the note marks it as heard).
        val note = """[{"source":"SubSense","problem":"blocked"}]"""
        push(snapshot(subtitles = listOf(subtitle, other), notes = note, extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the snapshot is heard") { controller.subtitlePanelNote != null }
        assertEquals(2, player.items.size)
        assertEquals(emptyList(), chosen(), "the start subtitle is not the viewer's pick")
        controller.release()
    }

    @Test
    fun `the viewer's pick or Off wins over the start subtitle and goes back to the core`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        controller.selectText(TRACK_OFF)
        until("Off goes to the core") { chosen() == listOf(null) }
        // The note marks the snapshot as heard.
        val note = """[{"source":"SubSense","problem":"blocked"}]"""
        push(snapshot(subtitles = listOf(subtitle, other), notes = note, extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the snapshot is heard") { controller.subtitlePanelNote != null }
        assertEquals(1, player.items.size, "the start subtitle turned on after Off")
        // A pick of a file not loaded yet: it loads, and its address goes to the core.
        controller.selectText("late:$other")
        until("the pick is side-loaded") { player.items.size == 2 }
        until("the pick goes to the core") { chosen() == listOf(null, other) }
        controller.release()
    }

    @Test
    fun `moving on to the next version says which, and a failed pick offers the next`() {
        val (_, controller) = open()
        push(
            snapshot(
                subtitles = emptyList(),
                phase = "resolving",
                extra = "\"trying\":{\"key\":\"k2\",\"label\":\"1080p WEB\"},",
            ),
        )
        until("the next version is named") { controller.status == "Trying the next version: 1080p WEB" }
        // The Versions button follows the version tried, and its subtitles are still to come.
        assertEquals("k2", controller.playingKey)
        assertEquals("Searching", controller.subtitlesDetail())
        push(snapshot(phase = "failed", extra = "\"offer\":{\"key\":\"k3\",\"label\":\"720p BluRay\"},"))
        until("the playback failed") { controller.stage == PlaybackController.Stage.Failed }
        assertEquals(OtherVersion("k3", "720p BluRay"), controller.offer)
        controller.release()
    }

    @Test
    fun `Subtitles says Searching while the core lists, and the count once the core says the listing is done`() {
        // Tracks arrive while slower sources are still asked: the core says pending until all answered.
        val (_, controller) = open(snapshot(subtitles = listOf(subtitle, other), pending = true))
        until("the list is heard") { controller.text.size == 3 }
        assertEquals("Searching", controller.subtitlesDetail())
        push(snapshot(subtitles = listOf(subtitle, other), pending = false))
        until("the end of the listing is heard") { !controller.subtitlesPending }
        assertEquals("Off, 2 found", controller.subtitlesDetail())
        controller.release()
    }

    @Test
    fun `a source that has what a first frame needs and shows none moves on as a decode failure`() {
        val (player, controller) = open()
        // Media3 holds 20 s of the file and still waits: the decoder gives no frame and raises nothing.
        player.media(video264, 30_000_000L)
        player.buffered(20_000)
        runFor(700)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(NO_PICTURE_MS))
        until("the decode failure goes to the core") { failOvers().isNotEmpty() }
        val sent = failOvers().single()["command"]!!.jsonObject
        assertEquals("decode", sent["kind"]?.jsonPrimitive?.content)
        assertEquals("This device cannot decode the video", sent["error"]?.jsonPrimitive?.content)
        assertEquals(1, player.stops)
        // The next source waits afresh: the last one's wait does not carry over.
        push(snapshot(url = "http://127.0.0.1:7780/video2.mp4"))
        until("the next source loads") { player.items.size == 2 }
        player.media(video264, 30_000_000L)
        player.buffered(20_000)
        runFor(1_200)
        assertEquals(1, failOvers().size)
        controller.release()
    }

    @Test
    fun `a pause before the first frame, or a first frame, keeps the picture net out of the way`() {
        val (player, controller) = open()
        controller.pause()
        player.media(video264, 30_000_000L)
        player.buffered(20_000)
        runFor(700)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(NO_PICTURE_MS))
        runFor(1_200)
        assertEquals(emptyList(), failOvers(), "paused by the viewer")
        controller.play()
        player.renderFirstFrame()
        runFor(700)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(NO_PICTURE_MS))
        runFor(1_200)
        assertEquals(emptyList(), failOvers(), "a frame was shown")
        controller.release()
    }

    @Test
    fun `a subtitle longer than the video ends the play at the video's end`() {
        val (player, controller) = open()
        controller.selectText("late:$subtitle")
        until("the subtitle is side-loaded") { player.items.size == 2 }
        player.media(video264, 52_209_000L)
        player.state(Player.STATE_READY)
        player.renderFirstFrame()
        until("it plays") { player.isPlaying }
        // The video's end passed a second ago; the subtitle has cues for minutes more.
        player.position(53_300)
        until("the play ends") { controller.stage == PlaybackController.Stage.Ended }
        assertFalse(player.playWhenReady)
        controller.release()
    }

    /** A player that records the items it is given and raises the errors a test hands it. */
    private class FakePlayer : SimpleBasePlayer(Looper.getMainLooper()) {
        class Item(
            val uri: String,
            val subtitles: List<String>,
        )

        val items = ArrayList<Item>()
        var stops = 0
        private var tracks = Tracks.EMPTY
        private var durationUs = C.TIME_UNSET

        var current =
            State
                .Builder()
                .setAvailableCommands(
                    Player.Commands
                        .Builder()
                        .addAllCommands()
                        .build(),
                ).setPlaybackState(Player.STATE_IDLE)
                .build()

        override fun getState(): State = current

        fun renderFirstFrame() {
            current = current.buildUpon().setNewlyRenderedFirstFrame(true).build()
            invalidateState()
            current = current.buildUpon().setNewlyRenderedFirstFrame(false).build()
        }

        /** What the loaded item turned out to hold: its tracks and length. */
        fun media(
            tracks: Tracks,
            durationUs: Long,
        ) {
            this.tracks = tracks
            this.durationUs = durationUs
            current = current.buildUpon().setPlaylist(current.playlist.map { data(it.mediaItem, it.uid) }).build()
            invalidateState()
        }

        fun buffered(ms: Long) {
            current = current.buildUpon().setContentBufferedPositionMs(PositionSupplier.getConstant(ms)).build()
            invalidateState()
        }

        fun position(ms: Long) {
            current = current.buildUpon().setContentPositionMs(ms).build()
            invalidateState()
        }

        private fun data(
            item: MediaItem,
            uid: Any,
        ) = MediaItemData
            .Builder(uid)
            .setMediaItem(item)
            .setTracks(tracks)
            .setDurationUs(durationUs)
            .build()

        /** The playback state as Media3 reports it ([Player.STATE_BUFFERING], [Player.STATE_READY]). */
        fun state(playbackState: Int) {
            current = current.buildUpon().setPlaybackState(playbackState).build()
            invalidateState()
        }

        fun fail(error: PlaybackException) {
            current =
                current
                    .buildUpon()
                    .setPlaybackState(Player.STATE_IDLE)
                    .setPlayerError(error)
                    .build()
            invalidateState()
        }

        override fun handleSetMediaItems(
            mediaItems: List<MediaItem>,
            startIndex: Int,
            startPositionMs: Long,
        ): ListenableFuture<*> {
            for (m in mediaItems) {
                val local = m.localConfiguration!!
                items += Item(local.uri.toString(), local.subtitleConfigurations.map { it.uri.toString() })
            }
            current =
                current
                    .buildUpon()
                    .setPlaylist(
                        mediaItems.mapIndexed {
                            i,
                            m,
                            ->
                            data(m, "item$i")
                        },
                    ).setCurrentMediaItemIndex(0)
                    .setContentPositionMs(startPositionMs.coerceAtLeast(0L))
                    .setPlaybackState(Player.STATE_IDLE)
                    .setPlayerError(null)
                    .build()
            return Futures.immediateVoidFuture()
        }

        override fun handlePrepare(): ListenableFuture<*> {
            current =
                current
                    .buildUpon()
                    .setPlayerError(null)
                    .setPlaybackState(if (current.playlist.isEmpty()) Player.STATE_IDLE else Player.STATE_BUFFERING)
                    .build()
            return Futures.immediateVoidFuture()
        }

        override fun handleStop(): ListenableFuture<*> {
            stops++
            current =
                current
                    .buildUpon()
                    .setPlaybackState(Player.STATE_IDLE)
                    .setPlayerError(null)
                    .build()
            return Futures.immediateVoidFuture()
        }

        override fun handleRemoveMediaItems(
            fromIndex: Int,
            toIndex: Int,
        ): ListenableFuture<*> {
            current =
                current
                    .buildUpon()
                    .setPlaylist(emptyList())
                    .setPlaybackState(Player.STATE_IDLE)
                    .build()
            return Futures.immediateVoidFuture()
        }

        override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
            current =
                current
                    .buildUpon()
                    .setPlayWhenReady(playWhenReady, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
                    .build()
            return Futures.immediateVoidFuture()
        }

        override fun handleSetTrackSelectionParameters(
            trackSelectionParameters: TrackSelectionParameters,
        ): ListenableFuture<*> {
            current = current.buildUpon().setTrackSelectionParameters(trackSelectionParameters).build()
            return Futures.immediateVoidFuture()
        }

        override fun handleRelease(): ListenableFuture<*> = Futures.immediateVoidFuture()
    }
}
