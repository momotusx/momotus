package app.marquee.ui.tv

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.media3.common.text.Cue
import androidx.media3.ui.SubtitleView
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.CHOOSE_ANOTHER
import app.marquee.player.LOADING_TAG
import app.marquee.player.LoadingLayer
import app.marquee.player.PlaybackController
import app.marquee.player.SUBTITLE_PICKER_TAG
import app.marquee.player.SubtitleDelay
import app.marquee.player.TitleMark
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
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals

/**
 * The TV player's loading screen (plans/player-polish.md 1.2: the mark alone, nothing focused, the remote's Back
 * cancels) and its subtitle picker in place.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvLoadingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

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
    private val player = PlayingPlayer()

    /** The addon files the controller fetched; every one holds a line from 1:00.5 to 1:03. */
    private val fetches = CopyOnWriteArrayList<String>()
    private val controller =
        PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime(), SubtitleDelay()) {
            fetches += it
            "1\n00:01:00,500 --> 00:01:03,000\nWho goes there\n".toByteArray()
        }

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** What the app sent the core. */
    private val sent = CopyOnWriteArrayList<String>()

    /** The core's playback view, ready with a stream, as a scripted core answers its subscribe. */
    private fun readyView(snapshot: String = READY): PlaybackView {
        client.attach { text ->
            sent += text
            val msg = CoreJson.parseToJsonElement(text).jsonObject
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
        return PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() }
    }

    /** The full picker: Subtitles opens its strip, More languages the picker (plans/player-bar.md D5). */
    private fun openPicker(subtitles: String = "Subtitles, Off, 2 languages") {
        compose.onNodeWithContentDescription(subtitles, substring = true).performClick()
        compose.onNodeWithText("More languages").performClick()
        compose.waitForIdle()
    }

    @After
    fun stop() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `until the first frame the loading screen alone shows, with nothing to focus, and Back leaves`() {
        var left = 0
        controller.showStatus("Starting 1080p, 1.5 GB from Torrentio")
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, { left++ }) } }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        // The title said once, by the layer (its mark is the name here, with no logo), the status under it; no header,
        // no controls.
        compose.onNodeWithContentDescription("Loading Sintel").assertExists()
        assertEquals(0, compose.onAllNodesWithText("Sintel").fetchSemanticsNodes().size)
        assertEquals(1, compose.onAllNodesWithText("Starting 1080p, 1.5 GB from Torrentio").fetchSemanticsNodes().size)
        assertEquals(0, compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().size)
        assertEquals(
            0,
            compose.onAllNodes(isFocused() and hasClickAction()).fetchSemanticsNodes().size,
            "nothing to press by mistake",
        )
        compose.onNodeWithTag(LOADING_TAG).performKeyInput { pressKey(Key.Back) }
        compose.runOnIdle { assertEquals(1, left) }
    }

    @Test
    fun `while the first source buffers the loading screen stays alone, and Left and Right do not seek`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView()) }
        // The core answers on its own thread and the controller follows on the main looper: both are let run here.
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        assertEquals(
            0,
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().size,
            "no controls",
        )
        compose.onNodeWithTag(LOADING_TAG).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.runOnIdle { assertEquals(60_000L, player.currentPosition, "the start point stays") }
        assertEquals(0, compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().size)
    }

    @Test
    fun `More languages opens the picker with the focus in it, and Back gives the focus back to Subtitles`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        openPicker("Subtitles")
        compose.onNodeWithTag(SUBTITLE_PICKER_TAG).assertExists()
        // No subtitles here: Off, chosen, takes the focus.
        compose.onNodeWithContentDescription("Off").assertIsFocused()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.Back) }
        compose.waitForIdle()
        compose.onNodeWithTag(SUBTITLE_PICKER_TAG).assertDoesNotExist()
        compose.onNodeWithContentDescription("Subtitles", substring = true).assertIsFocused()
    }

    @Test
    fun `Left in the open picker stays in it, with the controls composed under it`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView(WITH_SUBTITLES)) }
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        openPicker()
        compose.onNodeWithContentDescription("Off").assertIsFocused()
        // The transport sits at the left under the picker: Left from Off and from a language goes nowhere.
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.onNodeWithContentDescription("Off").assertIsFocused()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNodeWithContentDescription("English, 1 option").assertIsFocused()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.onNodeWithContentDescription("English, 1 option").assertIsFocused()
        compose.onNode(isFocused() and hasAnyAncestor(hasTestTag(SUBTITLE_PICKER_TAG))).assertExists()
    }

    @Test
    fun `a screen reader hears where the fill is going, not each frame of its easing`() {
        var fill by mutableFloatStateOf(0.1f)
        compose.setContent {
            MarqueeTheme(Form.Tv) { LoadingLayer("Sintel", TitleMark.Name, null, { fill }, emptyList()) }
        }
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        fill = 0.7f
        Snapshot.sendApplyNotifications()
        compose.mainClock.advanceTimeByFrame()
        val heard =
            compose
                .onNodeWithTag(LOADING_TAG)
                .fetchSemanticsNode()
                .config[SemanticsProperties.ProgressBarRangeInfo]
        assertEquals(0.7f, heard.current, "the easing toward 0.7 has only begun")
        compose.mainClock.autoAdvance = true
    }

    /** The cues the picture's subtitle view holds now. */
    private fun drawn(): List<String> {
        fun find(view: View): SubtitleView? =
            view as? SubtitleView ?: (view as? ViewGroup)?.let { group ->
                (0 until group.childCount).firstNotNullOfOrNull { find(group.getChildAt(it)) }
            }
        val view = requireNotNull(find(compose.activity.window.decorView)) { "no subtitle view" }
        // Media3 keeps the cues in a private field (as the cue tests read them).
        val field = SubtitleView::class.java.getDeclaredField("cues").apply { isAccessible = true }

        @Suppress("UNCHECKED_CAST")
        return (field.get(view) as List<Cue>).map { it.text.toString() }
    }

    @Test
    fun `an addon pick draws over the picture at once, follows the position and the delay, leaves with Off`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView(WITH_SUBTITLES)) }
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        openPicker()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
        compose
            .onNodeWithContentDescription(
                "English, 1 option",
            ).assertIsFocused()
            .performKeyInput { pressKey(Key.Enter) }
        // The file is parsed off the main thread, and the controller takes it on the main looper.
        compose.waitUntil(10_000) {
            shadowOf(Looper.getMainLooper()).idle()
            controller.addonOn
        }
        compose.runOnIdle { assertEquals(listOf("http://127.0.0.1:7781/en.srt"), fetches) }
        // 1:00, before the line; then 1:01, in it.
        compose.waitForIdle()
        assertEquals(emptyList(), drawn())
        compose.runOnIdle { controller.seekTo(61_000) }
        compose.waitUntil(10_000) { drawn() == listOf("Who goes there") }
        // A second later: the line has not started yet at 1:01.
        compose.runOnIdle { controller.setSubtitleDelay(1_000) }
        compose.waitUntil(10_000) { drawn().isEmpty() }
        compose.runOnIdle { controller.selectText("off") }
        compose.waitForIdle()
        assertEquals(emptyList(), drawn())
        assertEquals(1, player.items, "the item was set again")
        assertEquals(1, player.prepares, "the item was prepared again")
    }

    @Test
    fun `the picture takes over from the loading screen`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertDoesNotExist()
    }

    @Test
    fun `a stall over the picture tells the core, and the core's Choose another version shows over the frame`() {
        var versions = 0
        compose.setContent {
            MarqueeTheme(Form.Tv) { TvPlayer(controller, false, { versions++ }, null, { _, _ -> }, {}) }
        }
        compose.runOnIdle { controller.attach(readyView(READY_CHOOSE)) }
        val until = { stage: PlaybackController.Stage ->
            compose.waitUntil(10_000) {
                Snapshot.sendApplyNotifications()
                shadowOf(Looper.getMainLooper()).idle()
                controller.stage == stage
            }
        }
        until(PlaybackController.Stage.Buffering)
        compose.runOnIdle { player.open() }
        until(PlaybackController.Stage.Playing)
        compose.runOnIdle { player.stall() }
        until(PlaybackController.Stage.Buffering)
        compose.waitUntil(10_000) { sent.any { "\"stalled\"" in it && "\"stalled\":true" in it } }
        compose.waitForIdle()
        compose.onNodeWithText(CHOOSE_ANOTHER).assertExists()
        // The first D-pad key focuses it; OK opens Versions.
        compose.onRoot().performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        compose.onNodeWithText(CHOOSE_ANOTHER).performKeyInput { pressKey(Key.DirectionCenter) }
        compose.runOnIdle { assertEquals(1, versions) }
        compose.runOnIdle { player.resume() }
        compose.waitUntil(10_000) { sent.any { "\"stalled\":false" in it } }
    }

    @Test
    fun `a start that becomes ready while Versions is over the player stays still and silent`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView()) }
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        // Versions opens over the player while the start is still under way, as the screen marks it.
        compose.runOnIdle { controller.covered = true }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) {
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Paused || controller.stage == PlaybackController.Stage.Playing
        }
        compose.runOnIdle {
            assertEquals(PlaybackController.Stage.Paused, controller.stage, "nothing plays under Versions")
            assertEquals(false, player.playWhenReady, "and nothing makes a sound")
        }
    }

    @Test
    fun `a stall that ends by a pause tells the core it is over`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView()) }
        val until = { stage: PlaybackController.Stage ->
            compose.waitUntil(10_000) {
                Snapshot.sendApplyNotifications()
                shadowOf(Looper.getMainLooper()).idle()
                controller.stage == stage
            }
        }
        until(PlaybackController.Stage.Buffering)
        compose.runOnIdle { player.open() }
        until(PlaybackController.Stage.Playing)
        compose.runOnIdle { player.stall() }
        until(PlaybackController.Stage.Buffering)
        compose.waitUntil(10_000) { sent.any { "\"stalled\":true" in it } }
        // The viewer pauses during the stall: no data came, but the stall is over for the core's clock.
        compose.runOnIdle { controller.pause() }
        compose.waitUntil(10_000) { sent.any { "\"stalled\":false" in it } }
    }

    private val out = System.getenv("MARQUEE_SHOTS")?.let(::File)

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `shot - Choose another version over a stalled frame`() {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { controller.attach(readyView(READY_CHOOSE)) }
        val until = { stage: PlaybackController.Stage ->
            compose.waitUntil(10_000) {
                Snapshot.sendApplyNotifications()
                shadowOf(Looper.getMainLooper()).idle()
                controller.stage == stage
            }
        }
        until(PlaybackController.Stage.Buffering)
        compose.runOnIdle { player.open() }
        until(PlaybackController.Stage.Playing)
        compose.runOnIdle { player.stall() }
        until(PlaybackController.Stage.Buffering)
        compose.waitForIdle()
        compose.mainClock.advanceTimeBy(2_000)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(
            out,
            "tv-player-stalled-choose.jpg",
        ).outputStream().use { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }
}

/** The same, with the core offering another version (a stall of 30 s over the picture). */
private const val READY_CHOOSE =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[],"chooseAnother":true}"""

/** A playback snapshot with its stream, for [readyView]. */
private const val READY =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[]}"""

/** A stream with an English track from OpenSubtitles and a Turkish one from SubSense, as the core lists them. */
private const val WITH_SUBTITLES =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[""" +
        """{"url":"http://127.0.0.1:7781/en.srt","lang":"eng","language":"en","languageName":"English",""" +
        """"nativeName":"English","name":"CAKES, WEB 1080p","source":"OpenSubtitles v3","forced":false,""" +
        """"hearingImpaired":false},""" +
        """{"url":"http://127.0.0.1:7781/tr.srt","lang":"tur","language":"tr","languageName":"Turkish",""" +
        """"nativeName":"Türkçe","name":"SubSense","source":"SubSense","forced":false,"hearingImpaired":false}]}"""
