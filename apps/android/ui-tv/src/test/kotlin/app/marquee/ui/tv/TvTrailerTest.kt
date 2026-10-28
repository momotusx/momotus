package app.marquee.ui.tv

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.TrailerState
import app.marquee.player.FullTrailer
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The full-screen trailer on a TV (plans/trailer-inline.md 7): the bar has the focus and Left and Right seek. */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvTrailerTest {
    @get:Rule
    val compose = createComposeRule()

    private val state =
        TrailerState().apply {
            seconds = 30f
            length = 150f
        }
    private val seeks = mutableListOf<Float>()
    private var toggles = 0

    private fun show(failed: Boolean = false) {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                FullTrailer(
                    title = "Sintel",
                    state = state,
                    failed = failed,
                    loud = true,
                    onToggle = { toggles += 1 },
                    onSeek = {
                        seeks += it
                        state.seconds = it
                    },
                    onLoud = {},
                    onClose = {},
                ) {}
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun `the bar has the focus and Left and Right seek ten seconds once let go, never past the trailer`() {
        show()
        compose.onNodeWithContentDescription("Position").assertIsFocused()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.runOnIdle { assertEquals(listOf(40f, 50f, 40f), seeks) }
        // Near the end it stops 2 s short of the close; near the start, at the start.
        compose.runOnIdle { state.seconds = 145f }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        compose.runOnIdle { state.seconds = 4f }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
        compose.runOnIdle { assertEquals(listOf(40f, 50f, 40f, 148f, 0f), seeks) }
    }

    @Test
    fun `a held Right previews growing steps and seeks once, when it is let go`() {
        show()
        compose.onNode(isFocused()).performKeyInput {
            keyDown(Key.DirectionRight)
            advanceEventTime(2_000)
            keyUp(Key.DirectionRight)
        }
        compose.runOnIdle {
            assertEquals(1, seeks.size, "one seek for the whole hold: $seeks")
            assertTrue(seeks[0] > 40f, "the step grew while held: $seeks")
        }
    }

    @Test
    fun `select on the bar and the remote's play key pause and play, fast forward seeks from anywhere`() {
        show()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionCenter) }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.MediaPlayPause) }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.MediaFastForward) }
        compose.runOnIdle {
            assertEquals(2, toggles)
            assertEquals(listOf(40f), seeks)
        }
    }

    @Test
    fun `a trailer that cannot play says so and leaves only Close`() {
        show(failed = true)
        compose.onNodeWithText("This trailer can't play here.").assertExists()
        compose.onNode(hasText("Close").and(hasClickAction())).assertIsFocused()
        compose.onNode(hasText("Pause").and(hasClickAction())).assertIsNotEnabled()
    }
}
