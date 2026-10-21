package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
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
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * On a phone, keyboard focus is the ring alone (design/DECISION.md, Keyboard focus): the focused target
 * neither scales, rises nor changes its fill, and a button or row there does not invert.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w400dp-h800dp-mdpi")
class PhoneFocusTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a focused phone target draws the ring outside its edge and changes nothing inside it`() {
        var focused by mutableStateOf(false)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                Box(Modifier.testTag("area").background(Marquee.colors.bg).padding(PAD.dp)) {
                    Box(
                        Modifier
                            .size(SIZE.dp)
                            .marqueeFocus(8.dp, forced = focused)
                            .background(Marquee.colors.raised),
                    )
                }
            }
        }
        val before = compose.onNodeWithTag("area").captureToImage().toPixelMap()
        focused = true
        compose.waitForIdle()
        val after = compose.onNodeWithTag("area").captureToImage().toPixelMap()
        // Inside, away from the corners and the edge the ring meets.
        val core = PAD + 10 until PAD + SIZE - 10
        var inside = 0
        var ring = 0
        var far = 0
        for (x in 0 until after.width) {
            for (y in 0 until after.height) {
                val changed = !close(before[x, y].toArgb(), after[x, y].toArgb())
                val dx = maxOf(PAD - x, x - (PAD + SIZE - 1), 0)
                val dy = maxOf(PAD - y, y - (PAD + SIZE - 1), 0)
                val out = maxOf(dx, dy)
                when {
                    out == 0 && x in core && y in core -> if (changed) inside++

                    out in 1..4 -> if (changed) ring++

                    // The 4 dp ring ends at 4; a scaled target would push it past 5.
                    out > 5 -> if (changed) far++
                }
            }
        }
        assertEquals(0, inside, "pixels inside the focused target that changed")
        assertTrue(ring > 0, "no ring outside the focused target")
        assertEquals(0, far, "pixels beyond the ring that changed (a scale, a rise or a shadow)")
    }

    @Test
    fun `a focused button or row does not invert on a phone`() {
        var palette: Palette? = null
        compose.setContent { MarqueeTheme(Form.Phone) { palette = focusPalette(focused = true) } }
        compose.waitForIdle()
        assertEquals(Tokens.dark, palette)
    }

    @Test
    fun `a focused button or row inverts on TV`() {
        var palette: Palette? = null
        compose.setContent { MarqueeTheme(Form.Tv) { palette = focusPalette(focused = true) } }
        compose.waitForIdle()
        assertEquals(Tokens.dark.inverted(), palette)
    }

    private fun close(
        a: Int,
        b: Int,
    ): Boolean = (0..2).all { shift -> abs((a shr (shift * 8) and 0xFF) - (b shr (shift * 8) and 0xFF)) <= 2 }

    private companion object {
        const val PAD = 24
        const val SIZE = 100
    }
}
