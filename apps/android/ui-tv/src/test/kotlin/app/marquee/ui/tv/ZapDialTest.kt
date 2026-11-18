package app.marquee.ui.tv

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import app.marquee.core.model.Dial
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.ZapDial
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The zap dial on TV (plans/zap.md 2.3): focus moving across it changes nothing, OK on another step does. */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class ZapDialTest {
    @get:Rule
    val compose = createComposeRule()

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.mainClock.advanceTimeByFrame()
    }

    @Test
    fun `a sweep across the dial changes nothing, OK on a step changes it once`() {
        val pressed = mutableListOf<Dial>()
        compose.setContent { MarqueeTheme(Form.Tv) { ZapDial(Dial.Near, { pressed += it }) } }
        compose
            .onNodeWithContentDescription(
                "Same: what you watch",
            ).performSemanticsAction(SemanticsActions.RequestFocus)
        press(Key.DirectionRight)
        press(Key.DirectionRight)
        compose.onNodeWithContentDescription("Far: what you never open").assertIsFocused()
        compose.mainClock.advanceTimeBy(2_000)
        assertEquals(emptyList(), pressed)
        // OK on the step already chosen zaps nothing.
        press(Key.DirectionLeft)
        press(Key.DirectionCenter)
        assertEquals(emptyList(), pressed)
        press(Key.DirectionRight)
        press(Key.DirectionCenter)
        compose.waitForIdle()
        assertEquals(listOf(Dial.Far), pressed)
    }
}
