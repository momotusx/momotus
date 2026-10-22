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
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.io.IOException
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

    /** What the loopback serves for each addon file; a file not here answers 502. */
    private val files =
        mutableMapOf(
            subtitle to "1\n00:00:01,000 --> 00:00:03,000\nMerhaba\n\n2\n00:00:05,000 --> 00:00:06,000\nHoşça kal\n",
            other to "1\n00:00:01,000 --> 00:00:02,000\nSelam\n",
        )

    /** The addon files the controller fetched, in order. */
    private val fetches = CopyOnWriteArrayList<String>()

    private val fetch: suspend (String) -> ByteArray = { url ->
        fetches += url
        files[url]?.toByteArray() ?: throw IOException("502 for $url")
    }

    /** What the addon track on draws at [ms]. */
    private fun PlaybackController.drawnAt(ms: Long) = addonCues(ms).map { it.text.toString() }

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
    private fun open(
        first: String = snapshot,
        delay: SubtitleDelay = SubtitleDelay(),
        /** The first snapshot has the stream: wait for it to load. */
        loads: Boolean = true,
    ): Pair<FakePlayer, PlaybackController> {
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
        val controller =
            PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime(), delay, fetch)
        controller.attach(PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() })
        if (loads) until("the stream loads") { player.items.size == 1 }
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
     * Lets the main looper, the core's state thread and Compose's snapshot changes catch up until [check] holds.
     * It returns the moment it does; the bound is only there to end a hang: 10 s ran out under a loaded machine
     * (load average 15 to 34, the gate beside other builds), so nothing here waits on a tight clock.
     */
    private fun until(
        what: String,
        check: () -> Boolean,
    ) {
        val start = System.nanoTime()
        while (System.nanoTime() - start < UNTIL_BOUND_NS) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            if (check()) return
            Thread.sleep(5)
        }
        fail("waited ${UNTIL_BOUND_NS / 1_000_000_000} s and it never happened: $what")
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
        files.remove(subtitle)
        val (player, controller) = open()
        // The viewer picks the Turkish file; the addon answers 502 for it.
        controller.selectText("late:$subtitle")
        until("the track is dropped") { controller.text.none { it.id == "late:$subtitle" } }
        assertEquals(listOf(subtitle), fetches)
        assertFalse(controller.addonOn)
        assertEquals(1, player.items.size, "the item was set again")
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
        until("the subtitle is drawn") { controller.addonOn }
        push(snapshot(notes = """[{"source":"SubSense","problem":"unreachable"}]"""))
        until(
            "the note is said",
        ) { controller.notice == "SubSense did not answer. Try its subtitles again in a moment." }
        assertEquals(null, controller.status, "the note came with the waiting mark")
        assertEquals(1, player.items.size, "the item was set again")
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
        until("the subtitle is drawn") { controller.addonOn }
        // The core answered empty WebVTT and took the file out of the list, with a note.
        push(snapshot(subtitles = listOf(other), notes = """[{"source":"SubSense","problem":"trackFailed"}]"""))
        until("subtitles go off") { !controller.addonOn }
        assertEquals(1, player.items.size, "the item was set again")
        assertTrue(
            player.current.trackSelectionParameters.disabledTrackTypes
                .contains(C.TRACK_TYPE_TEXT),
            "the file's own tracks came on",
        )
        // One message, the note's: no "did not load" on top.
        assertEquals("A subtitle file from SubSense could not be read. Pick another one.", controller.notice)
        assertEquals(null, controller.status, "the note came with the waiting mark")
        assertEquals(0, player.stops)
        assertTrue(failOvers().isEmpty())
        assertTrue(controller.text.none { it.id == "late:$subtitle" }, "the file is still offered")
        assertFalse(controller.subtitleOn)
        controller.release()
    }

    @Test
    fun `a new source sets its own item once, and the addon pick goes over to its file in the same language`() {
        val (player, controller) = open()
        controller.selectText("late:$subtitle")
        until("the subtitle is drawn") { controller.addonOn }
        // A failover: a new loopback URL, and the subtitle files under new addresses.
        val failedOver =
            snapshot(subtitles = listOf(other))
                .replace(video, "http://127.0.0.1:7780/second.mp4")
                .replace("\"attempt\":1", "\"attempt\":2")
        push(failedOver)
        until("the new source loads") { player.items.lastOrNull()?.uri == "http://127.0.0.1:7780/second.mp4" }
        until("the Turkish file of the new source is drawn") { controller.drawnAt(1_500) == listOf("Selam") }
        repeat(20) { until("the looper idles") { true } }
        assertEquals(2, player.items.size, "the old item was set again before the new one")
        assertTrue(controller.text.any { it.id == "late:$other" && it.selected })
        controller.release()
    }

    @Test
    fun `a buffer that runs dry during play is logged as a stall`() {
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
            controller.release()
        } finally {
            CoreLog.sink = { _, _, _ -> }
        }
    }

    @Test
    fun `the core's start subtitle turns on once by itself, and nothing goes back`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        push(snapshot(subtitles = listOf(subtitle, other), extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the start subtitle is drawn") { controller.drawnAt(2_000) == listOf("Merhaba") }
        // Once: the same snapshot again changes nothing (the note marks it as heard).
        val note = """[{"source":"SubSense","problem":"blocked"}]"""
        push(snapshot(subtitles = listOf(subtitle, other), notes = note, extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the snapshot is heard") { controller.subtitlePanelNote != null }
        assertEquals(1, player.items.size)
        assertEquals(listOf(subtitle), fetches)
        assertEquals(emptyList(), chosen(), "the start subtitle is not the viewer's pick")
        controller.release()
    }

    @Test
    fun `once the file opens, its own track in the start subtitle's language takes over, and nothing goes back`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        push(snapshot(subtitles = listOf(subtitle, other), extra = "\"autoSubtitle\":\"$subtitle\","))
        until("the start subtitle is drawn") { controller.addonOn }
        // The file turns out to hold a Turkish track (and a forced one, which never takes over).
        player.media(withText(listOf(true, false), "tr"), 600_000_000L)
        until("the file's track is on") {
            player.current.trackSelectionParameters.overrides.keys
                .any { it.id == "text:1" }
        }
        assertEquals(emptyList(), chosen(), "still the start pick: the core does not hear of it")
        // The viewer's own choice stays: Off, and a file that lists its track again does not turn it back on.
        controller.selectText(TRACK_OFF)
        until("Off goes to the core") { chosen() == listOf(null) }
        player.media(withText(listOf(false), "tr"), 600_000_000L)
        runFor(600)
        assertTrue(C.TRACK_TYPE_TEXT in player.current.trackSelectionParameters.disabledTrackTypes, "Off holds")
        controller.release()
    }

    private var textGroupsMade = 0

    /** The H.264 video and a text track per [forced] inside the file, in [language]; groups `text:0`, `text:1` on. */
    private fun withText(
        forced: List<Boolean>,
        language: String,
    ) = Tracks(
        video264.groups +
            forced.map {
                val n = textGroupsMade++
                Tracks.Group(
                    TrackGroup(
                        "text:$n",
                        Format
                            .Builder()
                            .setId("$n")
                            .setLanguage(language)
                            .setSampleMimeType(MimeTypes.TEXT_VTT)
                            .setSelectionFlags(if (it) C.SELECTION_FLAG_FORCED else 0)
                            .build(),
                    ),
                    false,
                    intArrayOf(C.FORMAT_HANDLED),
                    booleanArrayOf(false),
                )
            },
    )

    @Test
    fun `the loading fill follows the core's start, never back, then the player's buffer, and a stall starts over`() {
        val start = { progress: Float, caption: String? ->
            "\"start\":{\"stage\":\"connecting\",\"peers\":3,\"haveBytes\":0,\"needBytes\":8000000,\"bytesPerS\":0," +
                "\"source\":1,\"sources\":1,\"progress\":$progress${caption?.let { ",\"caption\":\"$it\"" } ?: ""}},"
        }
        val (player, controller) = open(snapshot(phase = "resolving", extra = start(0.2f, null)), loads = false)
        until("the core's start is heard") { controller.loadingFill == 0.2f }
        push(snapshot(phase = "resolving", extra = start(0.5f, "Finding peers, 3 so far")))
        until("it grows") { controller.loadingFill == 0.5f }
        assertEquals("Finding peers, 3 so far", controller.start?.caption)
        push(snapshot(phase = "resolving", extra = start(0.4f, null)))
        until("the caption goes") { controller.start?.caption == null }
        assertEquals(0.5f, controller.loadingFill, "never back within one start")
        // The URL: the start is gone, and the buffer fills the rest against the start threshold.
        push(snapshot())
        until("the stream loads") { player.items.size == 1 }
        assertEquals(null, controller.start)
        player.buffered(START_BUFFER_MS / 2L)
        until("the buffer is heard") { controller.loadingFill > 0.5f }
        assertEquals(0.75f, controller.loadingFill, 1e-4f)
        // The picture, then a stall: the fill starts over from the buffer, to the rebuffer threshold.
        player.state(Player.STATE_READY)
        player.renderFirstFrame()
        until("the picture shows") { controller.picture }
        player.state(Player.STATE_BUFFERING)
        until("the stall starts the fill over") {
            controller.stage == PlaybackController.Stage.Buffering && controller.loadingFill < 0.5f
        }
        until("the buffer is heard") { controller.loadingFill > 0f }
        assertEquals(START_BUFFER_MS / 2f / REBUFFER_MS, controller.loadingFill, 1e-4f)
        controller.release()
    }

    @Test
    fun `after a picture, a failed source or a moved server fills the next start from nothing`() {
        val (player, controller) = open()
        player.buffered(START_BUFFER_MS.toLong())
        until("the buffer fills the logo") { controller.loadingFill == 1f }
        player.state(Player.STATE_READY)
        player.renderFirstFrame()
        until("the picture shows") { controller.picture }
        // Twenty minutes in, the source breaks: the loading screen of the next one is not full from the start.
        player.fail(httpError(video, 503))
        until("the next source is asked for") { failOvers().size == 1 }
        assertFalse(controller.picture)
        assertEquals(0f, controller.loadingFill)
        assertTrue(controller.starting)
        // The next source plays; then the stream server moves while the app was away.
        push(snapshot(url = "http://127.0.0.1:7780/next.mp4"))
        until("the next source loads") { player.items.size == 2 }
        player.buffered(START_BUFFER_MS.toLong())
        until("its buffer fills the logo") { controller.loadingFill == 1f }
        player.renderFirstFrame()
        until("its picture shows") { controller.picture }
        controller.serverMoved()
        assertEquals(0f, controller.loadingFill)
        assertFalse(controller.picture)
        controller.release()
    }

    @Test
    fun `a start the app leaves goes on paused and plays once the viewer is back, a play already showing does not`() {
        val (player, controller) = open(snapshot(phase = "resolving"), loads = false)
        until("the start is heard") { controller.subtitlesPending }
        assertTrue(controller.starting)
        StartService.keep(RuntimeEnvironment.getApplication(), "Sintel")
        controller.onStop()
        // Away: the core's view stays open and its stream loads, with no sound.
        push(snapshot())
        until("the stream loads") { player.items.size == 1 }
        assertFalse(sent.any { it["op"]?.jsonPrimitive?.content == "unsubscribe" }, "the core's view stays open")
        player.state(Player.STATE_READY)
        until("Ready, paused") { controller.stage == PlaybackController.Stage.Paused }
        assertFalse(player.playWhenReady, "no sound in the background")
        player.renderFirstFrame()
        until("the start is over") { !controller.starting }
        until("the background start is let go") { !StartService.keeping }
        // Back: it plays at its point.
        controller.onStart()
        until("it plays") { player.playWhenReady }
        // Leaving while it plays pauses it, and it stays paused when the viewer is back.
        controller.onStop()
        assertFalse(player.playWhenReady)
        controller.onStart()
        runFor(100)
        assertFalse(player.playWhenReady, "a play the viewer left stays paused")
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
        until("the pick is drawn") { controller.addonOn }
        until("the pick goes to the core") { chosen() == listOf(null, other) }
        assertEquals(1, player.items.size)
        controller.release()
    }

    /** The renderer's error for a decoder that did not start, with how it supports the file. */
    private fun decoderDidNotStart(support: Int) =
        ExoPlaybackException.createForRenderer(
            IllegalStateException("decoder init"),
            "MediaCodecVideoRenderer",
            0,
            Format.Builder().setSampleMimeType(MimeTypes.VIDEO_H264).build(),
            support,
            false,
            PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
        )

    @Test
    fun `a decoder that did not start for a file past it is decode, for a file it takes other`() {
        val (player, controller) = open()
        player.fail(decoderDidNotStart(C.FORMAT_EXCEEDS_CAPABILITIES))
        until("the failure goes to the core") { failOvers().size == 1 }
        assertEquals(
            "decode",
            failOvers()
                .last()["command"]!!
                .jsonObject["kind"]
                ?.jsonPrimitive
                ?.content,
        )
        // The core's next source of the same play: the decoder is busy this time.
        push(snapshot(url = "http://127.0.0.1:7780/video2.mp4"))
        until("the next source loads") { player.items.size == 2 }
        player.fail(decoderDidNotStart(C.FORMAT_HANDLED))
        until("the second failure goes to the core") { failOvers().size == 2 }
        assertEquals(
            "other",
            failOvers()
                .last()["command"]!!
                .jsonObject["kind"]
                ?.jsonPrimitive
                ?.content,
        )
        controller.release()
    }

    @Test
    fun `a Try again the core took says Trying again, also when the play fails again at once`() {
        val (_, controller) = open()
        push(snapshot(phase = "failed"))
        until("the playback failed") { controller.stage == PlaybackController.Stage.Failed }
        controller.retry()
        until("retry goes to the core") { commands("retry").size == 1 }
        // The core took it and the play failed again at once: only the count tells the snapshots apart.
        push(snapshot(phase = "failed", extra = "\"retries\":1,"))
        until("the card says so") { controller.tryingAgain }
        assertEquals(PlaybackController.Stage.Failed, controller.stage)
        until("for a moment only") { !controller.tryingAgain }
        // A Try again whose play starts over says it while the core resolves.
        controller.retry()
        push(snapshot(phase = "resolving", extra = "\"retries\":2,"))
        until("the play starts over") { controller.stage == PlaybackController.Stage.Opening }
        assertEquals(TRYING_AGAIN, controller.status)
        controller.release()
    }

    @Test
    fun `a version that could not be opened says so, and Try again opens it again once`() {
        val controller = PlaybackController(FakePlayer(), scope, "Sintel", null, null, SystemClock.elapsedRealtime())
        var opens = 0
        controller.cannotOpen("that version is no longer listed") { opens++ }
        assertEquals(PlaybackController.Stage.Failed, controller.stage)
        assertEquals(CANNOT_OPEN, controller.problem)
        assertEquals("that version is no longer listed", controller.openError)
        assertTrue(controller.reopenable)
        controller.reopen()
        assertEquals(1, opens)
        assertEquals(PlaybackController.Stage.Opening, controller.stage)
        assertEquals(null, controller.openError)
        assertFalse(controller.reopenable)
        // A second press before it failed again does nothing more.
        controller.reopen()
        assertEquals(1, opens)
        controller.release()
    }

    @Test
    fun `a failed play offers Try again, which sends retry and opens again`() {
        val (_, controller) = open()
        assertEquals(false, controller.retryable)
        push(snapshot(phase = "failed"))
        until("the playback failed") { controller.stage == PlaybackController.Stage.Failed }
        assertTrue(controller.retryable)
        assertEquals("None of the sources could be played", controller.problem)
        controller.retry()
        until("retry goes to the core") { commands("retry").size == 1 }
        // A source that fails again at once leaves the core's snapshot as it was: the card and Try again stay.
        runFor(200)
        assertEquals(PlaybackController.Stage.Failed, controller.stage)
        assertTrue(controller.retryable)
        // The core starts over: the card goes.
        push(snapshot(phase = "resolving"))
        until("the play starts over") { controller.stage == PlaybackController.Stage.Opening }
        assertEquals(null, controller.problem)
        assertEquals(false, controller.retryable)
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
        assertEquals("This version could not be played", controller.problem)
        controller.release()
    }

    @Test
    fun `Subtitles counts what is in while the core lists, with a mark that goes when the listing is done`() {
        // Nothing in yet: Searching, no mark.
        val (_, controller) = open(snapshot(subtitles = emptyList(), pending = true))
        until("the snapshot is heard") { controller.subtitlesPending }
        assertEquals("Searching", controller.subtitlesDetail())
        assertFalse(controller.subtitlesStillListing())
        // Tracks in while slower sources are still asked: the count, with the mark.
        push(snapshot(subtitles = listOf(subtitle, other), pending = true))
        until("the list is heard") { controller.text.size == 3 }
        assertEquals("Off, 1 language", controller.subtitlesDetail())
        assertTrue(controller.subtitlesStillListing())
        // The core says the listing is done: the mark goes.
        push(snapshot(subtitles = listOf(subtitle, other), pending = false))
        until("the end of the listing is heard") { !controller.subtitlesPending }
        assertEquals("Off, 1 language", controller.subtitlesDetail())
        assertFalse(controller.subtitlesStillListing())
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
    fun `a pick, Off and every delay step on an addon track are instant and never set the item again`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        // With subtitles off there is no track to time.
        controller.setSubtitleDelay(500)
        assertEquals(0L, controller.subtitleDelayMs)
        controller.selectText("late:$subtitle")
        until("the subtitle is drawn") { controller.addonOn }
        // Its cues follow the position: 1 to 3 s and 5 to 6 s of the file.
        assertEquals(listOf("Merhaba"), controller.drawnAt(2_000))
        assertEquals(emptyList(), controller.drawnAt(4_000))
        assertEquals(listOf("Hoşça kal"), controller.drawnAt(5_500))
        // A second later: what the file has at 2.5 s shows at 3.5 s, at once.
        controller.setSubtitleDelay(1_000)
        assertEquals(listOf("Merhaba"), controller.drawnAt(3_500))
        assertEquals(emptyList(), controller.drawnAt(1_500))
        // Two seconds earlier: the file's 5.5 s at 3.5 s.
        controller.setSubtitleDelay(-2_000)
        assertEquals(listOf("Hoşça kal"), controller.drawnAt(3_500))
        // Within a minute either way.
        controller.setSubtitleDelay(90_000)
        assertEquals(SUBTITLE_MAX_MS, controller.subtitleDelayMs)
        controller.setSubtitleDelay(1_000)
        // Off clears the cues; the same track on again is back at once with its delay, its file not fetched again.
        controller.selectText(TRACK_OFF)
        assertFalse(controller.addonOn)
        assertEquals(emptyList(), controller.drawnAt(2_000))
        controller.selectText("late:$subtitle")
        assertTrue(controller.addonOn, "on again at once")
        assertEquals(listOf("Merhaba"), controller.drawnAt(3_500))
        runFor(1_000)
        assertEquals(listOf(subtitle), fetches)
        assertEquals(1, player.items.size, "the item was set again")
        assertEquals(1, player.prepares, "the item was prepared again")
        controller.release()
    }

    @Test
    fun `each track keeps its own delay, and Off and on again keeps it`() {
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle, other)))
        controller.selectText("late:$subtitle")
        until("the subtitle is drawn") { controller.addonOn }
        controller.setSubtitleDelay(1_500)
        // Another track starts on time.
        controller.selectText("late:$other")
        assertEquals(0L, controller.subtitleDelayMs)
        until("the other is drawn") { controller.drawnAt(1_500) == listOf("Selam") }
        // Back to the first: its delay comes back.
        controller.selectText("late:$subtitle")
        assertEquals(1_500L, controller.subtitleDelayMs)
        assertEquals(listOf("Merhaba"), controller.drawnAt(3_000))
        controller.selectText(TRACK_OFF)
        controller.selectText("late:$subtitle")
        assertEquals(1_500L, controller.subtitleDelayMs)
        runFor(1_000)
        assertEquals(1, player.items.size, "the item was set again")
        controller.release()
    }

    @Test
    fun `a delay on the file's own track goes to its renderer at once, and never sets the item again`() {
        val delay = SubtitleDelay()
        val (player, controller) = open(snapshot(subtitles = listOf(subtitle)), delay = delay)
        player.media(withOwnTrack, 10_000_000L)
        controller.selectText(OWN)
        until("the file's track is on") { controller.subtitleOn }
        controller.setSubtitleDelay(1_500)
        assertEquals(1_500L, delay.ms, "the renderer reads the new delay")
        // An addon track keeps its own; back on the file's track, its delay comes back.
        controller.selectText("late:$subtitle")
        until("the subtitle is drawn") { controller.addonOn }
        assertEquals(0L, controller.subtitleDelayMs)
        controller.selectText(OWN)
        assertEquals(1_500L, delay.ms)
        runFor(1_000)
        assertEquals(1, player.items.size, "the item was set again")
        assertEquals(1, player.prepares, "the item was prepared again")
        controller.release()
    }

    /** The video and a Turkish text track of the file's own, on: the track's id in the menu is [OWN]. */
    private val withOwnTrack =
        Tracks(
            video264.groups +
                Tracks.Group(
                    TrackGroup(
                        "text:own",
                        Format
                            .Builder()
                            .setId("0")
                            .setSampleMimeType(MimeTypes.APPLICATION_SUBRIP)
                            .setLanguage("tr")
                            .build(),
                    ),
                    false,
                    intArrayOf(C.FORMAT_HANDLED),
                    booleanArrayOf(true),
                ),
        )

    @Test
    fun `a stream server that moved while the app was away opens the play again at its new address`() {
        val moved = "http://127.0.0.1:7790/video.mp4"
        val subscribes =
            java.util.concurrent.atomic
                .AtomicInteger()
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    val n = subscribes.incrementAndGet()
                    """{"sub":${6 + n},"snapshot":${if (n == 1) snapshot else snapshot(url = moved)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val player = FakePlayer()
        val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
        controller.attach(PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() })
        until("the stream loads") { player.items.size == 1 }
        player.position(42_000)
        controller.serverMoved()
        until("the play opens again at the new address") { player.items.size == 2 }
        assertEquals(moved, player.items.last().uri)
        assertEquals(42_000L, player.items.last().startMs, "opened again where it was")
        assertEquals(2, subscribes.get())
        assertNotEquals("Playing a different source", controller.notice)
        controller.release()
    }

    @Test
    fun `a player under another page stays paused, whatever asks it to play`() {
        val (player, controller) = open()
        controller.play()
        assertTrue(player.playWhenReady)
        controller.covered = true
        assertFalse(player.playWhenReady)
        // The remote's Play key reaches the media session, which plays the player itself.
        player.play()
        until("paused again") { !player.playWhenReady }
        controller.covered = false
        controller.play()
        assertTrue(player.playWhenReady)
        controller.release()
    }

    /** A player that records the items it is given and raises the errors a test hands it. */
    private companion object {
        /** The menu's id of [withOwnTrack]'s text track: the second group, its first track. */
        const val OWN = "1:0"
    }

    private class FakePlayer : SimpleBasePlayer(Looper.getMainLooper()) {
        class Item(
            val uri: String,
            val subtitles: List<String>,
            val startMs: Long = 0L,
        )

        val items = ArrayList<Item>()
        var stops = 0
        var prepares = 0
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
                items +=
                    Item(local.uri.toString(), local.subtitleConfigurations.map { it.uri.toString() }, startPositionMs)
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
            prepares++
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

/** How long [SubtitleFailureTest]'s waits may take before they count as a hang. */
private const val UNTIL_BOUND_NS = 120_000_000_000L
