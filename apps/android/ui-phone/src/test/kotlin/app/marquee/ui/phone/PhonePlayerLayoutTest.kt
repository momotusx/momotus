package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.media3.ui.SubtitleView
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.CUE_LINE
import app.marquee.player.CUE_LINES
import app.marquee.player.CUE_SHRINK
import app.marquee.player.PlaybackController
import app.marquee.player.SUBTITLES_TAG
import app.marquee.player.subtitleScale
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
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone player's layout on a wide, low phone in landscape (the owner's rule): the transport in the middle
 * over the bar, the options row under the bar as one group (never spread edge to edge), and the subtitles at
 * the foot, lifted between the controls and the title bar while the controls show, never over a control.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w900dp-h396dp")
class PhonePlayerLayoutTest {
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
    private val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    private fun playing(size: SubtitleSize = SubtitleSize.Medium) {
        controller.subtitleSize = size
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
    }

    private fun bounds(description: String) =
        compose.onNodeWithContentDescription(description).fetchSemanticsNode().boundsInRoot

    private fun controlsHidden() {
        compose.waitUntil(10_000) {
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().isEmpty()
        }
        compose.waitForIdle()
    }

    /** Two lines of cues as the subtitle view will draw them: its own foot and text size, read from the view. */
    private class Cues(
        val top: Float,
        val foot: Float,
        val text: Float,
        val bottom: Float,
    )

    private fun cues(): Cues {
        fun find(view: View): SubtitleView? =
            view as? SubtitleView ?: (view as? ViewGroup)?.let { group ->
                (0 until group.childCount).firstNotNullOfOrNull { find(group.getChildAt(it)) }
            }
        val view = requireNotNull(find(compose.activity.window.decorView)) { "no subtitle view" }

        fun field(name: String) =
            SubtitleView::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .getFloat(view)
        val height = view.height.toFloat()
        val bottom = field("bottomPaddingFraction")
        val text = field("defaultTextSize")
        val foot = height * (1 - bottom)
        return Cues(foot - CUE_LINES * CUE_LINE * text * height, foot, text, bottom)
    }

    /** The cues' size with the controls hidden: the system caption size (1 here) times the profile's. */
    private fun full(size: SubtitleSize) = SubtitleView.DEFAULT_TEXT_SIZE_FRACTION * subtitleScale(size)

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
        val area = compose.onNodeWithTag(SUBTITLES_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertEquals(root, area, "the subtitles keep the whole picture area, so they keep their size")
        val shown = cues()
        val transport = bounds("Back 10 seconds")
        assertTrue(shown.foot <= transport.top, "over the controls: ${shown.foot} against ${transport.top}")
        assertEquals(full(SubtitleSize.Medium), shown.text, "two lines at Medium fit: full size")
        // The controls hide while it plays: the subtitles go back to the foot, at full size.
        controlsHidden()
        val hidden = cues()
        assertEquals(SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION, hidden.bottom, 1e-5f, "at the foot")
        assertEquals(full(SubtitleSize.Medium), hidden.text, "full size")
    }

    @Test
    fun `two lines at the largest size never meet a control, at 85% while the controls show`() {
        playing(SubtitleSize.ExtraLarge)
        val shown = cues()
        val transport = bounds("Back 10 seconds")
        val over =
            listOf(
                bounds("Back").bottom,
                bounds("Picture in picture").bottom,
                compose
                    .onNodeWithText("Sintel")
                    .fetchSemanticsNode()
                    .boundsInRoot.bottom,
            ).max()
        assertTrue(shown.foot <= transport.top, "over the transport: ${shown.foot} against ${transport.top}")
        assertTrue(shown.top >= over, "under the title bar: two lines reach ${shown.top}, its controls end at $over")
        assertEquals(full(SubtitleSize.ExtraLarge) * CUE_SHRINK, shown.text, 1e-6f, "at 85%")
        controlsHidden()
        assertEquals(full(SubtitleSize.ExtraLarge), cues().text, "back to full size once the controls hide")
    }
}
