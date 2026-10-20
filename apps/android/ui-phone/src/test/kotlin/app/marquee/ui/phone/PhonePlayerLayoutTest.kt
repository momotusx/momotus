package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.SUBTITLES_TAG
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
import kotlin.math.abs
import kotlin.test.assertTrue

/**
 * The phone player's layout on a wide phone in landscape (the owner's rule): the transport in the middle
 * over the bar, the options row under the bar as one group (never spread edge to edge), and the subtitles
 * at the foot, lifted just above the controls while they show.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w900dp-h410dp")
class PhonePlayerLayoutTest {
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

    private fun playing() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
    }

    private fun bounds(description: String) =
        compose.onNodeWithContentDescription(description).fetchSemanticsNode().boundsInRoot

    @Test
    fun `transport over the bar, the options as one group under it, in the middle`() {
        playing()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val transport = bounds("Back 10 seconds")
        val bar = bounds("Position")
        val actions = compose.onNodeWithTag(ACTIONS_TAG).fetchSemanticsNode().boundsInRoot
        assertTrue(transport.bottom <= bar.top, "the transport sits over the bar")
        assertTrue(bar.bottom <= actions.top, "the options row sits under the bar")
        assertTrue(
            actions.width < root.width * 0.6f,
            "the options are a group, not spread across ${root.width}: ${actions.width}",
        )
        assertTrue(actions.left >= bar.left && actions.right <= bar.right, "inside the bar's margins: $actions, $bar")
        assertTrue(abs(actions.center.x - root.center.x) < 2f, "the group is in the middle")
    }

    @Test
    fun `the subtitles sit just above the controls while they show, at the foot when they hide`() {
        playing()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val transport = bounds("Back 10 seconds")
        val shown = compose.onNodeWithTag(SUBTITLES_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue(shown.bottom <= transport.top + 1f, "over the controls: ${shown.bottom} against ${transport.top}")
        // The controls hide while it plays: the subtitles go back to the foot.
        compose.waitUntil(10_000) {
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().isEmpty()
        }
        val hidden = compose.onNodeWithTag(SUBTITLES_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue(abs(hidden.bottom - root.bottom) < 1f, "at the foot: ${hidden.bottom} against ${root.bottom}")
    }
}
