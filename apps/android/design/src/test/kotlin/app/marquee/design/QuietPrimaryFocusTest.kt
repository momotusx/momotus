package app.marquee.design

import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.requestFocus
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** A quiet primary keeps its node, so a focus held there stays, and on TV it counts as a focused button. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class QuietPrimaryFocusTest {
    @get:Rule
    val compose = createComposeRule()

    private var quiet by mutableStateOf(false)

    private var buttons: MutableIntState? = null

    private fun show() =
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                buttons = LocalButtonFocus.current
                PrimaryButton("Save", {}, quiet = quiet)
            }
        }

    @Test
    fun `a focus held on the button stays when it turns quiet and back`() {
        show()
        compose.onNodeWithText("Save").requestFocus()
        compose.onNodeWithText("Save").assertIsFocused()
        quiet = true
        compose.waitForIdle()
        compose.onNodeWithText("Save").assertIsFocused()
        quiet = false
        compose.waitForIdle()
        compose.onNodeWithText("Save").assertIsFocused()
    }

    @Test
    fun `on TV a focused quiet primary counts as a focused button, a focused plain one does not`() {
        show()
        compose.onNodeWithText("Save").requestFocus()
        compose.onNodeWithText("Save").assertIsFocused()
        compose.runOnIdle { assertEquals(0, buttons!!.intValue) }
        quiet = true
        compose.waitForIdle()
        // The page's white primary yields to it, so its own white is the one white.
        compose.runOnIdle { assertEquals(1, buttons!!.intValue) }
    }
}
