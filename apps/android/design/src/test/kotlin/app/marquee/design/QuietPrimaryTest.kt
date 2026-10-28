package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertTrue

/** A quiet primary draws as a secondary: one white per screen, with the question's safe button holding it. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class QuietPrimaryTest {
    @get:Rule
    val compose = createComposeRule()

    private var quiet by mutableStateOf(false)

    private fun show() =
        compose.setContent {
            MarqueeTheme(Form.Phone) {
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
}
