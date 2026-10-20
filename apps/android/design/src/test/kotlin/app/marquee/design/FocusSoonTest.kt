package app.marquee.design

import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.requestFocus
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** A screen's deferred first focus never takes back a press the viewer made before it came (the web's d8aa39f1). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class FocusSoonTest {
    @get:Rule
    val compose = createComposeRule()

    private val first = FocusRequester()

    private fun show() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Column {
                    SecondaryButton("First", {}, focusRequester = first)
                    SecondaryButton("Second", {})
                }
                LaunchedEffect(Unit) { focusSoon(first, frames = 60) }
            }
        }
    }

    @Test
    fun `the first focus lands when nothing was pressed`() {
        show()
        compose.onNodeWithText("First").assertIsFocused()
    }

    @Test
    fun `a press before the first focus came keeps the viewer's move`() {
        compose.mainClock.autoAdvance = false
        show()
        // The window gave Second its focus and the viewer pressed, both before the frame the first focus waits for.
        compose.onNodeWithText("Second").requestFocus()
        KeyPresses.pressed()
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNodeWithText("Second").assertIsFocused()
    }
}
