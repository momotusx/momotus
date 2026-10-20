package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
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
import kotlin.test.assertEquals

/** Skip intro is a plain white pill: a ring in the accent that never goes away reads as focus stuck on it. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class SkipButtonTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the Skip pill has no accent edge while it is not focused`() {
        var accent = Color.Unspecified
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                accent = Marquee.colors.accent
                Box(Modifier.testTag("skip").background(Marquee.colors.bg).padding(16.dp)) {
                    SkipButton(Segment(SegmentKind.Intro, 30_000, 75_000, "TheIntroDB"), {})
                }
            }
        }
        val pixels = compose.onNodeWithTag("skip").captureToImage().toPixelMap()
        val want = accent.toArgb()
        var count = 0
        for (x in 0 until pixels.width) {
            for (y in 0 until pixels.height) {
                if (close(pixels[x, y].toArgb(), want)) count++
            }
        }
        assertEquals(0, count, "pixels in the accent colour")
    }

    private fun close(
        a: Int,
        b: Int,
    ): Boolean = (0..2).all { shift -> abs((a shr (shift * 8) and 0xFF) - (b shr (shift * 8) and 0xFF)) <= 16 }
}
