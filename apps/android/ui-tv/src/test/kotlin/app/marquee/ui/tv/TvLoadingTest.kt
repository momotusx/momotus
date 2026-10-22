package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.LOADING_TAG
import app.marquee.player.PlaybackController
import app.marquee.player.SUBTITLE_PICKER_TAG
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
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
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
    val compose = createComposeRule()

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
    private val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** The core's playback view, ready with a stream, as a scripted core answers its subscribe. */
    private fun readyView(snapshot: String = READY): PlaybackView {
        client.attach { text ->
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
    fun `Subtitles opens the picker with the focus in it, and Back gives the focus back to Subtitles`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.onNodeWithContentDescription("Subtitles", substring = true).performClick()
        compose.waitForIdle()
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
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.waitForIdle()
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
    fun `the picture takes over from the loading screen`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertDoesNotExist()
    }
}

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
