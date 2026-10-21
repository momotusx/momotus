package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.LOADING_TAG
import app.marquee.player.PlaybackController
import app.marquee.player.SUBTITLE_PICKER_TAG
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
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

    @After
    fun stop() {
        controller.release()
        scope.cancel()
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
    fun `the picture takes over from the loading screen`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertDoesNotExist()
    }
}
