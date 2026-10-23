package app.marquee.design

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** The phone's badges with large text: each box grows to hold its word, and the word keeps the phone's 11 sp floor. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w411dp-h891dp")
class BadgeRowTest {
    @get:Rule
    val compose = createComposeRule()

    private val badges =
        listOf("4K" to BadgeLook.Filled, "Dolby Vision" to BadgeLook.Outlined, "5.1" to BadgeLook.Plain)

    private fun layout(text: String): TextLayoutResult {
        val results = mutableListOf<TextLayoutResult>()
        compose
            .onNodeWithText(text, useUnmergedTree = true)
            .fetchSemanticsNode()
            .config[SemanticsActions.GetTextLayoutResult]
            .action
            ?.invoke(results)
        return results.single()
    }

    private fun scaled(scale: Float) {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalDensity provides Density(LocalDensity.current.density, scale)) {
                    BadgeRow(badges)
                }
            }
        }
        for ((text, _) in badges) {
            val result = layout(text)
            // One line that does not wrap reports its full paragraph width, so the height and the line's own width are read.
            assertFalse(result.didOverflowHeight, "$text spills out of its badge at $scale times the text size")
            val needs = result.multiParagraph.maxIntrinsicWidth
            assertTrue(result.size.width + 0.5f >= needs, "$text is cut at $scale times the text size")
            assertTrue(result.layoutInput.style.fontSize.value >= 11f, "$text is under the phone's 11 sp floor")
        }
    }

    @Test
    fun `at one and a half times the text size every badge holds its word`() = scaled(1.5f)

    /**
     * A TV badge on the full point canvas, measured with real text: its 23 sp line (28 tall) fits Apple's 31, so a word
     * that goes below the line ("Portuguese", "jpgy") is drawn whole (seen in a NATIVE render on 2026-10-23).
     */
    @Test
    @Config(qualifiers = "w1920dp-h1080dp")
    fun `a TV badge holds a word with descenders`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) { BadgeRow(listOf("Portuguese" to BadgeLook.Outlined, "jpgy" to BadgeLook.Filled)) }
        }
        for (text in listOf("Portuguese", "jpgy")) {
            assertFalse(layout(text).didOverflowHeight, "TV $text is cut by its badge")
        }
    }

    @Test
    fun `at twice the text size every badge holds its word`() = scaled(2f)
}
