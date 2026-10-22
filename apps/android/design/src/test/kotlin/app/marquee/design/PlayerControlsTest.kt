package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
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

/**
 * The player's controls sit over the picture with no scrim (the phone's Back 10 and Forward 10): inside it
 * ([LocalFocusRing]) a round button and a secondary keep the opaque `raised` fill, so a white frame never
 * swallows them. Elsewhere they take the 16 percent fill. One pixel a point, on a white frame.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w400dp-h800dp-mdpi")
class PlayerControlsTest {
    @get:Rule
    val compose = createComposeRule()

    /** The luma of the round button's fill, six points in from its left edge on its middle line. */
    private fun fillLuma(player: Boolean): Int {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalFocusRing provides player) {
                    Box(Modifier.testTag("frame").background(Color.White).padding(PAD.dp)) {
                        RoundButton(Icons.RotateCcw, "Back 10 seconds", {})
                    }
                }
            }
        }
        val shot = compose.onNodeWithTag("frame").captureToImage().toPixelMap()
        val c = shot[PAD + 6, shot.height / 2].toArgb()
        return ((c shr 16 and 0xFF) + (c shr 8 and 0xFF) + (c and 0xFF)) / 3
    }

    @Test
    fun `inside the player a round button keeps its opaque fill over a white frame`() {
        val luma = fillLuma(player = true)
        assertTrue(luma < 100, "the round button's fill over white: luma $luma")
    }

    @Test
    fun `elsewhere a round button is the 16 percent fill`() {
        val luma = fillLuma(player = false)
        assertTrue(luma > 240, "the round button's fill over white: luma $luma")
    }

    private companion object {
        const val PAD = 24
    }
}
