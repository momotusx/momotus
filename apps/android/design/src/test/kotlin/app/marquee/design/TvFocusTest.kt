package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
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
import kotlin.test.assertEquals

/**
 * On TV focus is the lift and the white invert, never a ring (design/DECISION.md, TV focus), the player's
 * controls over the picture ([LocalOverPicture]) included. On the 1920 canvas at one pixel a point.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp-mdpi")
class TvFocusTest {
    @get:Rule
    val compose = createComposeRule()

    /** The capture before and after a forced focus, for a 100 point target [PAD] points in. */
    private fun shots(overPicture: Boolean): Pair<IntArray, IntArray> {
        var focused by mutableStateOf(false)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalOverPicture provides overPicture) {
                    Box(Modifier.testTag("area").background(Marquee.colors.bg).padding(PAD.dp)) {
                        // Scale 1: nothing rises or casts a shadow, so only a stroke could reach outside.
                        Box(
                            Modifier
                                .size(SIZE.dp)
                                .marqueeFocus(8.dp, scale = 1f, forced = focused)
                                .background(Marquee.colors.raised),
                        )
                    }
                }
            }
        }
        val before = compose.onNodeWithTag("area").captureToImage().toPixelMap()
        focused = true
        compose.waitForIdle()
        val after = compose.onNodeWithTag("area").captureToImage().toPixelMap()
        val w = after.width
        // One pixel a point, so the target sits at [PAD, PAD + SIZE) in the capture.
        assertEquals(PAD * 2 + SIZE, w, "the capture is not one pixel a point")
        return IntArray(w * after.height) { before[it % w, it / w].toArgb() } to
            IntArray(w * after.height) { after[it % w, it / w].toArgb() }
    }

    private fun luma(c: Int) = ((c shr 16 and 0xFF) + (c shr 8 and 0xFF) + (c and 0xFF)) / 3

    /** Pixels outside the target that a forced focus made brighter: a ring would. */
    private fun ringed(overPicture: Boolean): Int {
        val (before, after) = shots(overPicture)
        val side = PAD * 2 + SIZE
        var brighter = 0
        for (i in after.indices) {
            val x = i % side
            val y = i / side
            val outside = x < PAD || x >= PAD + SIZE || y < PAD || y >= PAD + SIZE
            if (outside && luma(after[i]) > luma(before[i]) + 2) brighter++
        }
        return brighter
    }

    @Test
    fun `a focused TV target draws no stroke outside its fill`() {
        assertEquals(0, ringed(overPicture = false), "pixels outside the focused TV target that got brighter (a ring)")
    }

    @Test
    fun `over the picture, in the player, a focused TV target draws no ring either`() {
        assertEquals(0, ringed(overPicture = true), "pixels outside the focused player control that got brighter")
    }

    @Test
    fun `over the picture a TV focus still turns a button white with dark words`() {
        var inverted = false
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalOverPicture provides true) {
                    inverted = focusPalette(focused = true) == Marquee.colors.inverted()
                }
            }
        }
        compose.waitForIdle()
        assertEquals(true, inverted, "the player's focused control keeps its own palette (a ring look)")
    }

    private companion object {
        const val PAD = 48
        const val SIZE = 100
    }
}
