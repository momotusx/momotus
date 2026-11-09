package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PixelMap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.MarqueeTheme
import app.marquee.design.RoundButton
import app.marquee.player.ActionButton
import app.marquee.player.StripChip
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertTrue

/**
 * The TV player's own controls show focus as every TV control does (design/DECISION.md, TV focus): white with dark
 * words, never a ring. Measured on the 1920 canvas, one pixel a point, with the WCAG contrast formula.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp-mdpi")
class TvFocusInvertTest {
    @get:Rule
    val compose = createComposeRule()

    private fun contrast(
        a: Color,
        b: Color,
    ): Float {
        val (hi, lo) = listOf(a.luminance(), b.luminance()).sortedDescending()
        return (hi + 0.05f) / (lo + 0.05f)
    }

    /** The fill just inside a node's left edge, on its middle line. */
    private fun fill(
        shot: PixelMap,
        at: Rect,
    ) = shot[at.left.toInt() + 4, at.center.y.toInt()]

    /** The pixel in [at] that stands out most from [ground]: the ink of its words and glyphs. */
    private fun ink(
        shot: PixelMap,
        at: Rect,
        ground: Color,
    ): Float {
        var best = 1f
        for (x in at.left.toInt() until at.right.toInt()) {
            for (y in at.top.toInt() until at.bottom.toInt()) best = maxOf(best, contrast(shot[x, y], ground))
        }
        return best
    }

    private fun bounds(spoken: String) = compose.onNodeWithContentDescription(spoken).fetchSemanticsNode().boundsInRoot

    @Test
    fun `a focused strip chip turns white with dark words, apart from the checked chip unfocused`() {
        var shown by mutableStateOf("a")
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Row(
                    Modifier.background(Color.Black).padding(48.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    StripChip("English, AAC, stereo", {}, checked = true, spoken = "English", forceFocus = shown == "a")
                    StripChip("Turkish, AAC", {}, checked = false, spoken = "Turkish", forceFocus = shown == "b")
                }
            }
        }

        // The checked chip with the focused look, then the other chip with it: the checked chip is only checked.
        fun chosen(): Pair<Color, Float> {
            compose.mainClock.advanceTimeBy(1_000)
            val at = bounds("English")
            val shot = compose.onRoot().captureToImage().toPixelMap()
            val ground = fill(shot, at)
            return ground to ink(shot, at, ground)
        }
        val (focusedFill, label) = chosen()
        shown = "b"
        val (chosenFill, _) = chosen()
        assertTrue(
            contrast(focusedFill, chosenFill) >= 3f,
            "focus is a step of only ${contrast(focusedFill, chosenFill)}: $focusedFill vs $chosenFill",
        )
        assertTrue(label >= 4.5f, "the focused row's words read at $label:1")
    }

    @Test
    fun `a focused action turns white with dark words, over a white frame and a dark one`() {
        var frameColor by mutableStateOf(Color.White)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Box(Modifier.testTag("frame").background(frameColor).padding(48.dp)) {
                    ActionButton(Icons.Layers, "Versions", "720p", {}, forceFocus = true)
                }
            }
        }

        // The action's fill just inside its edge, its words' contrast on it, and the frame around it.
        fun look(): Triple<Color, Float, Color> {
            compose.mainClock.advanceTimeBy(1_000)
            val at = bounds("Versions, 720p")
            val shot = compose.onNodeWithTag("frame").captureToImage().toPixelMap()
            val frame = compose.onNodeWithTag("frame").fetchSemanticsNode().boundsInRoot
            val local = Rect(at.left - frame.left, at.top - frame.top, at.right - frame.left, at.bottom - frame.top)
            val ground = fill(shot, local)
            return Triple(ground, ink(shot, local, ground), shot[4, 4])
        }
        val (_, overWhite, _) = look()
        assertTrue(overWhite >= 4.5f, "the focused action's words read at $overWhite:1 over a white frame")
        frameColor = Color.Black
        val (fillOnDark, overDark, dark) = look()
        assertTrue(contrast(fillOnDark, dark) >= 3f, "the focused action is not white: ${contrast(fillOnDark, dark)}:1")
        assertTrue(overDark >= 4.5f, "the focused action's words read at $overDark:1 on its fill")
    }

    @Test
    fun `while an action holds the focus the white Play yields, so the screen keeps one white`() {
        var actionFocused by mutableStateOf(false)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Row(
                    Modifier.background(Color.Black).padding(48.dp),
                    horizontalArrangement = Arrangement.spacedBy(48.dp),
                ) {
                    RoundButton(Icons.Pause, "Pause", {}, main = true)
                    ActionButton(Icons.Layers, "Versions", "720p", {}, forceFocus = actionFocused)
                }
            }
        }

        // The middle of the Play circle, beside its glyph: its fill.
        fun play(): Color {
            compose.mainClock.advanceTimeBy(1_000)
            val at = bounds("Pause")
            return compose.onRoot().captureToImage().toPixelMap()[at.left.toInt() + 8, at.center.y.toInt()]
        }
        assertTrue(play().luminance() > 0.7f, "Play is the white main action while nothing else holds the focus")
        actionFocused = true
        assertTrue(play().luminance() < 0.3f, "Play stayed white beside the focused action: ${play()}")
    }
}
