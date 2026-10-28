package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** A quiet primary draws as a secondary (one white per screen) on the same node, so a focus held there stays. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class QuietPrimaryTest {
    @get:Rule
    val compose = createComposeRule()

    private var quiet by mutableStateOf(false)

    private var buttons: MutableIntState? = null

    private fun show(form: Form = Form.Phone) =
        compose.setContent {
            MarqueeTheme(form) {
                buttons = LocalButtonFocus.current
                Box(Modifier.background(Color.Black).padding(8.dp).testTag("frame")) {
                    PrimaryButton("Save", {}, quiet = quiet)
                }
            }
        }

    /** The button's fill, just inside its rounded left end, left of the label. */
    private fun fill(): Color {
        val shot = compose.onNodeWithTag("frame").captureToImage().toPixelMap()
        return shot[8 + 6, shot.height / 2]
    }

    @Test
    fun `a quiet primary is not white, and a plain one is`() {
        show()
        assertTrue(fill().luminance() > 0.7f, "the primary is the white pill")
        quiet = true
        compose.waitForIdle()
        assertTrue(fill().luminance() < 0.3f, "the quiet primary stayed white: ${fill()}")
    }

    @Test
    fun `a focus held on the button stays when it turns quiet and back`() {
        show()
        compose.onNodeWithText("Save").performSemanticsAction(SemanticsActions.RequestFocus)
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
        show(Form.Tv)
        compose.onNodeWithText("Save").performSemanticsAction(SemanticsActions.RequestFocus)
        compose.runOnIdle { assertEquals(0, buttons!!.intValue) }
        quiet = true
        compose.waitForIdle()
        // The page's white primary yields to it, so its own white is the one white.
        compose.runOnIdle { assertEquals(1, buttons!!.intValue) }
    }
}
