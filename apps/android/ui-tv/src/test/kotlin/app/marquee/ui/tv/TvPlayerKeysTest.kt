package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
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

/** The TV player's keys while the controls are hidden (journey review P2-3). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvPlayerKeysTest {
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

    /** Playing at one minute in, the controls hidden again. */
    private fun playingHidden() {
        compose.setContent {
            MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        // The controls hide after four seconds of real time (their timer is not on the test clock).
        compose.waitUntil(10_000) {
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().isEmpty()
        }
    }

    @Test
    fun `Left with the controls hidden goes back ten seconds at once and shows them`() {
        playingHidden()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.runOnIdle { assertEquals(50_000L, player.currentPosition) }
        compose.onNodeWithContentDescription("Back 10 seconds").assertExists()
    }

    @Test
    fun `Right with the controls hidden goes ahead ten seconds at once`() {
        playingHidden()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        compose.runOnIdle { assertEquals(70_000L, player.currentPosition) }
    }

    @Test
    fun `Down with the controls hidden only shows them`() {
        playingHidden()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
        compose.runOnIdle { assertEquals(60_000L, player.currentPosition) }
        compose.onNodeWithContentDescription("Back 10 seconds").assertExists()
    }
}
