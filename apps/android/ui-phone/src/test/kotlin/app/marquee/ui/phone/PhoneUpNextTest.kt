package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.model.NextEpisode
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext

/** Up Next on the phone: Cancel stops the next episode from starting by itself, for this playback only. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h900dp")
class PhoneUpNextTest {
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
    private val started = mutableListOf<String>()

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    /** Playing with autoplay on and a next episode, in the last seconds of a ten-minute file: the card shows. */
    private fun nearTheEnd() {
        controller.next = NextEpisode("tt1:1:2", "The Second", 1, 2)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhonePlayer(controller, true, false, {}, null, { next, _ -> started += next.videoId }, {}, {})
            }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == Stage.Playing }
        compose.runOnIdle { controller.seekTo(590_000) }
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Play now").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun ends() {
        compose.runOnIdle { player.finish() }
        compose.waitUntil(10_000) { controller.stage == Stage.Ended }
    }

    @Test
    fun `Cancel hides the card and the end of the episode does not start the next one`() {
        nearTheEnd()
        compose.onNodeWithText("Cancel").performClick()
        compose.onAllNodesWithText("Play now").assertCountEquals(0)
        ends()
        // The end card is the way on, and nothing started by itself.
        compose.onNodeWithText("Next episode").assertExists()
        // No eyebrow over the title (design/DECISION.md, Player).
        compose.onAllNodesWithText("Finished", ignoreCase = true).assertCountEquals(0)
        compose.runOnIdle { assertEquals(emptyList<String>(), started) }
    }

    @Test
    fun `without Cancel the end of the episode starts the next one`() {
        nearTheEnd()
        ends()
        compose.runOnIdle { assertEquals(listOf("tt1:1:2"), started) }
    }

    @Test
    fun `Play now starts the next one`() {
        nearTheEnd()
        compose.onNodeWithText("Play now").performClick()
        compose.runOnIdle { assertEquals(listOf("tt1:1:2"), started) }
    }

    @Test
    fun `Play again on the end card takes the cancel back for the next time round`() {
        nearTheEnd()
        compose.onNodeWithText("Cancel").performClick()
        ends()
        compose.onNodeWithText("Play again").performClick()
        compose.waitUntil(10_000) { controller.stage != Stage.Ended }
        compose.runOnIdle { controller.seekTo(590_000) }
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Cancel").fetchSemanticsNodes().isNotEmpty() }
    }
}
