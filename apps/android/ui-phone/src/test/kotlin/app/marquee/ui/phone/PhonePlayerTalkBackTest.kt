package app.marquee.ui.phone

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.accessibility.AccessibilityManager
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performSemanticsAction
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
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext

/** The phone player with TalkBack (journey review P2-7): the controls stay, and the picture can bring them back. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class PhonePlayerTalkBackTest {
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

    private fun exploring(on: Boolean) {
        val manager =
            RuntimeEnvironment.getApplication().getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        shadowOf(manager).setTouchExplorationEnabled(on)
    }

    private fun playing() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
    }

    private fun controlsGone() =
        compose.onAllNodesWithContentDescription("Picture in picture").fetchSemanticsNodes().isEmpty()

    @Test
    fun `with TalkBack on the controls do not hide by themselves`() {
        exploring(true)
        playing()
        // Six seconds on both clocks, past the four the controls wait while playing and their fade.
        compose.waitForIdle()
        val until = System.currentTimeMillis() + 6_000
        while (System.currentTimeMillis() < until) {
            compose.mainClock.advanceTimeBy(100)
            Thread.sleep(100)
        }
        compose.onNodeWithContentDescription("Picture in picture").assertExists()
    }

    @Test
    fun `the picture has an action that shows the controls again`() {
        exploring(false)
        playing()
        compose.waitUntil(10_000) { controlsGone() }
        val picture =
            SemanticsMatcher("has the Show controls action") {
                it.config.getOrNull(SemanticsActions.OnClick)?.label == SHOW_CONTROLS
            }
        compose.onNode(picture).performSemanticsAction(SemanticsActions.OnClick)
        compose.onNodeWithContentDescription("Picture in picture").assertExists()
    }
}
