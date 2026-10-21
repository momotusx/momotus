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
import kotlin.test.assertTrue

/**
 * On TV focus is the lift and the white invert, never a ring (design/DECISION.md, TV focus); inside the
 * player ([LocalFocusRing]) the v2 white ring stays until phase 5. On the 1920 canvas at one pixel a point.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp-mdpi")
class TvFocusTest {
    @get:Rule
    val compose = createComposeRule()

    /** The capture before and after a forced focus, for a 100 point target [PAD] points in. */
    private fun shots(ringed: Boolean): Pair<IntArray, IntArray> {
        var focused by mutableStateOf(false)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalFocusRing provides ringed) {
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

    @Test
    fun `a focused TV target draws no stroke outside its fill`() {
        val (before, after) = shots(ringed = false)
        val side = PAD * 2 + SIZE
        var brighter = 0
        for (i in after.indices) {
            val x = i % side
            val y = i / side
            val outside = x < PAD || x >= PAD + SIZE || y < PAD || y >= PAD + SIZE
            if (outside && luma(after[i]) > luma(before[i]) + 2) brighter++
        }
        assertEquals(0, brighter, "pixels outside the focused TV target that got brighter (a ring)")
    }

    @Test
    fun `inside the player a focused TV target keeps the white ring`() {
        val (_, after) = shots(ringed = true)
        val side = PAD * 2 + SIZE
        // The ring sits past a 4 point gap: 4 to 8 points outside the edge, here at 6 on the left side.
        val at = (PAD + SIZE / 2) * side + (PAD - 6)
        assertTrue(luma(after[at]) > 200, "no white ring in the player: luma ${luma(after[at])}")
    }

    private companion object {
        const val PAD = 48
        const val SIZE = 100
    }
}
