package app.marquee.design

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The scores as text: the meta line shows each score there is (today IMDb's alone), a tile's caption none. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class RatingsTest {
    @get:Rule
    val compose = createComposeRule()

    private fun count(text: String) =
        compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().size

    @Test
    fun `the meta line shows the score, or none, before the facts`() {
        var ratings by mutableStateOf(listOf("IMDb 8.8"))
        compose.setContent { MarqueeTheme(Form.Phone) { RatingLine(ratings, "2010 · 148 min") } }
        assertEquals(1, count("IMDb 8.8"))
        ratings = emptyList()
        compose.waitForIdle()
        assertEquals(0, count("IMDb 8.8"))
        compose.onNodeWithText("2010 · 148 min").assertExists()
    }

    @Test
    // Real text measure: Robolectric's stand-in fonts are a few pixels a word.
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `on a narrow meta line the facts are cut and both scores stay whole`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                Box(Modifier.width(220.dp)) {
                    RatingLine(listOf("IMDb 8.8", "RT 87%"), "2011–2019 · 148 min · Drama, Science Fiction, Mystery")
                }
            }
        }
        for (score in listOf("IMDb 8.8", "RT 87%")) {
            val node = compose.onNodeWithText(score, useUnmergedTree = true).fetchSemanticsNode()
            val results = mutableListOf<TextLayoutResult>()
            node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)
            val layout = results.single()
            assertTrue(layout.size.width + 0.5f >= layout.multiParagraph.maxIntrinsicWidth, "$score is cut")
        }
        // The facts gave way: cut, not the scores.
        val facts = compose.onNodeWithText("2011–2019", substring = true, useUnmergedTree = true).fetchSemanticsNode()
        val results = mutableListOf<TextLayoutResult>()
        facts.config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)
        assertTrue(results.single().hasVisualOverflow, "the facts were not cut")
    }

    @Test
    fun `a score is pure white over art and the text level on the page`() {
        var onArt by mutableStateOf(true)
        compose.setContent { MarqueeTheme(Form.Tv) { RatingsRow(listOf("IMDb 8.8"), onArt = onArt) } }
        val number = {
            compose
                .onNodeWithText("IMDb 8.8", useUnmergedTree = true)
                .fetchSemanticsNode()
                .config[SemanticsProperties.Text]
                .single()
                .spanStyles
                .last()
                .item.color
        }
        assertEquals(Color.White, number())
        onArt = false
        compose.waitForIdle()
        assertEquals(formPalette(Form.Tv).text, number())
    }

    @Test
    fun `the first score links when the title page gives a link`() {
        var opened = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) { RatingLine(listOf("IMDb 8.8", "RT 87%"), "2010", onFirst = { opened++ }) }
        }
        compose.onNodeWithText("RT 87%").assertHasNoClickAction()
        compose.onNodeWithText("IMDb 8.8").assertHasClickAction().performClick()
        assertEquals(1, opened)
    }

    @Test
    fun `a tile's caption is its name and year, the cover shows the scores, and they are still read with the title`() {
        var captioned by mutableStateOf(true)
        compose.setContent {
            MarqueeTheme(
                Form.Tv,
            ) {
                PosterTile(
                    "Inception",
                    null,
                    {},
                    caption = "2010",
                    ratings = listOf("IMDb 8.8"),
                    captioned = captioned,
                )
            }
        }
        assertEquals(0, count("IMDb 8.8"))
        assertEquals(1, count("2010"))
        // The scores are read with the title, after the year.
        compose.onNodeWithContentDescription("Inception").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.StateDescription,
                "2010, IMDb 8.8",
            ),
        )
        // The phone's Home posters are the art alone: no name, no year.
        captioned = false
        compose.waitForIdle()
        assertEquals(0, count("2010"))
    }

    @Test
    fun `a 16 to 9 card's caption has no score either, only its name and line`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                PosterTile(
                    "Tenzing",
                    null,
                    {},
                    shape = TileShape.Landscape,
                    caption = "2026",
                    ratings = listOf("IMDb 7.5"),
                )
            }
        }
        assertEquals(1, count("2026"))
        assertEquals(0, count("IMDb 7.5"))
        compose.onNodeWithContentDescription("Tenzing").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "2026, IMDb 7.5"),
        )
    }
}
