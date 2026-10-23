package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import app.marquee.player.SkipButton
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.test.assertEquals

/**
 * Skip intro is a plain white pill: unfocused, nothing is drawn around it (a ring, halo or shadow that
 * never goes away reads as focus stuck on it). Focused it stays the white pill: `buttonFill` in DesignTest.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class SkipButtonTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `nothing is drawn around the Skip pill while it is not focused`() {
        var bg = Color.Unspecified
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                bg = Marquee.colors.bg
                Box(Modifier.testTag("skip").background(Marquee.colors.bg).padding(48.dp)) {
                    SkipButton(Segment(SegmentKind.Intro, 30_000, 75_000, "TheIntroDB"), {})
                }
            }
        }
        // The pill's box inside the captured area, in pixels.
        val area = compose.onNodeWithTag("skip").fetchSemanticsNode().boundsInRoot
        val node = compose.onNode(hasClickAction()).fetchSemanticsNode().boundsInRoot
        val left = (node.left - area.left).roundToInt()
        val top = (node.top - area.top).roundToInt()
        val right = (node.right - area.left).roundToInt()
        val bottom = (node.bottom - area.top).roundToInt()
        val pixels = compose.onNodeWithTag("skip").captureToImage().toPixelMap()
        val page = bg.toArgb()
        var off = 0
        // All around the pill: nothing is said above it either.
        for (x in 0 until pixels.width) {
            for (y in 0 until pixels.height) {
                val inside = x in left - 1..right && y in top - 1..bottom
                if (!inside && !close(pixels[x, y].toArgb(), page)) off++
            }
        }
        assertEquals(0, off, "pixels around the unfocused pill that are not the page color")
    }

    @Test
    fun `Skip says what it skips and never where its timing came from`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) { SkipButton(Segment(SegmentKind.Intro, 30_000, 75_000, "TheIntroDB"), {}) }
        }
        compose.onNodeWithText("Skip intro").assertExists()
        compose.onAllNodesWithText("TheIntroDB", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Skip data", substring = true).assertCountEquals(0)
    }

    private fun close(
        a: Int,
        b: Int,
    ): Boolean = (0..2).all { shift -> abs((a shr (shift * 8) and 0xFF) - (b shr (shift * 8) and 0xFF)) <= 6 }
}
