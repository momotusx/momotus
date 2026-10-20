package app.marquee.ui.tv

import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.SecondaryButton
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * A title or Versions page's first focus waits for its target (it comes with the data), but never takes
 * back a move the viewer made before the target came (the web's d8aa39f1).
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class FirstFocusTest {
    @get:Rule
    val compose = createComposeRule()

    private val focus = FocusMemory()
    private var target by mutableStateOf<String?>(null)
    private var late by mutableStateOf(false)

    /** A page with A, then an action the memory does not track ("Show all"), then B once the data came. */
    private fun show() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Column(
                    Modifier.onPreviewKeyEvent {
                        if (it.type == KeyEventType.KeyDown) focus.interacted()
                        false
                    },
                ) {
                    SecondaryButton(
                        "A",
                        {},
                        focusRequester = focus.requesterFor("a"),
                        onFocusChange = focus.watch(null, "a"),
                    )
                    SecondaryButton("Show all", {})
                    if (late) {
                        SecondaryButton(
                            "B",
                            {},
                            focusRequester = focus.requesterFor("b"),
                            onFocusChange = focus.watch(null, "b"),
                        )
                    }
                }
                focus.FirstFocus(target)
            }
        }
    }

    @Test
    fun `a target that comes late takes the first focus when nothing was pressed`() {
        show()
        late = true
        target = "b"
        compose.onNodeWithText("B").assertIsFocused()
    }

    @Test
    fun `a target that comes after a press does not take the viewer's move back`() {
        target = "a"
        show()
        compose.onNodeWithText("A").assertIsFocused()
        compose.onNodeWithText("A").performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNodeWithText("Show all").assertIsFocused()
        // The data comes: the page's target changes after the viewer moved.
        late = true
        target = "b"
        compose.waitForIdle()
        compose.onNodeWithText("Show all").assertIsFocused()
    }
}
