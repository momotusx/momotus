package app.marquee.player

import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.TrackSelectionParameters
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.ExoPlaybackException
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
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

    /** The core's playback snapshot with these subtitle files and notes. */
    private fun snapshot(
        subtitles: List<String> = listOf(subtitle),
        notes: String = "[]",
    ) = """{"phase":"ready","url":"$video","key":"k1","attempt":1,"manifest":false,"subtitlesPending":false,""" +
        """"trakt":"off","subtitleNotes":$notes,"subtitles":[""" +
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

    /** Lets the main looper, the core's state thread and Compose's snapshot changes catch up until [check]. */
    private fun until(
        what: String,
        check: () -> Boolean,
    ) {
        repeat(400) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            if (check()) return
            Thread.sleep(5)
        }
        fail(what)
    }

    private fun failOvers() =
        sent.filter {
            it["command"]
                ?.jsonObject
                ?.get("type")
                ?.jsonPrimitive
                ?.content == "failOver"
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
        assertEquals("Subtitles from SubSense did not load", controller.status)

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
        ) { controller.status == "SubSense did not answer. Try its subtitles again in a moment." }
        assertEquals(2, player.items.size, "the item was set again")
        assertEquals("SubSense did not answer. Try its subtitles again in a moment.", controller.subtitlePanelNote)
        assertTrue(failOvers().isEmpty())
        controller.release()
    }

    @Test
    fun `a note while no core track is on is listed but not said`() {
        val (_, controller) = open()
        push(snapshot(notes = """[{"source":"SubSense","problem":"blocked"}]"""))
        until("the note is listed") { controller.subtitlePanelNote != null }
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
        assertEquals(video, player.items.last().uri)
        assertEquals(emptyList(), player.items.last().subtitles)
        assertTrue(
            player.current.trackSelectionParameters.disabledTrackTypes
                .contains(C.TRACK_TYPE_TEXT),
            "subtitles stayed on",
        )
        // One message, the note's: no "did not load" on top.
        assertEquals("A subtitle file from SubSense could not be read. Pick another one.", controller.status)
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

    /** A player that records the items it is given and raises the errors a test hands it. */
    private class FakePlayer : SimpleBasePlayer(Looper.getMainLooper()) {
        class Item(
            val uri: String,
            val subtitles: List<String>,
        )

        val items = ArrayList<Item>()
        var stops = 0

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
                            MediaItemData.Builder("item$i").setMediaItem(m).build()
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
