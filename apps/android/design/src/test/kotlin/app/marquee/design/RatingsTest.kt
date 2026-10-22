package app.marquee.design

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
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

/** IMDb and Rotten Tomatoes side by side: the meta line and a poster's caption show each score there is, and nothing else. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class RatingsTest {
    @get:Rule
    val compose = createComposeRule()

    private fun count(text: String) =
        compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().size

    @Test
    fun `the meta line shows both scores, one, or none before the facts`() {
        var ratings by mutableStateOf(listOf("IMDb 8.8", "RT 87%"))
        compose.setContent { MarqueeTheme(Form.Phone) { RatingLine(ratings, "2010 · 148 min") } }
        assertEquals(1, count("IMDb 8.8"))
        assertEquals(1, count("RT 87%"))
        // TalkBack says the name, not "R T".
        compose.onNodeWithContentDescription("Rotten Tomatoes 87 percent", useUnmergedTree = true).assertExists()
        compose.onNodeWithContentDescription("IMDb 8.8", useUnmergedTree = true).assertExists()
        // The Rotten Tomatoes addon's mark has no number.
        ratings = listOf("IMDb 8.8", "RT Fresh")
        compose.waitForIdle()
        assertEquals(1, count("RT Fresh"))
        compose.onNodeWithContentDescription("Rotten Tomatoes: Fresh", useUnmergedTree = true).assertExists()
        ratings = listOf("RT 87%")
        compose.waitForIdle()
        assertEquals(0, count("IMDb 8.8"))
        assertEquals(1, count("RT 87%"))
        ratings = emptyList()
        compose.waitForIdle()
        assertEquals(0, count("RT 87%"))
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
    fun `a poster's caption carries the scores under the art, read with the title, and a bare poster has none`() {
        var ratings by mutableStateOf(listOf("IMDb 8.8", "RT 87%"))
        var captioned by mutableStateOf(true)
        compose.setContent {
            MarqueeTheme(
                Form.Tv,
            ) { PosterTile("Inception", null, {}, caption = "2010", ratings = ratings, captioned = captioned) }
        }
        assertEquals(1, count("IMDb 8.8"))
        assertEquals(1, count("RT 87%"))
        val art = compose.onNodeWithContentDescription("Inception").fetchSemanticsNode().boundsInRoot
        val score = compose.onNodeWithText("IMDb 8.8", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue(score.top >= art.bottom, "the score sits on the art")
        // The scores are read with the title, after the year.
        compose.onNodeWithContentDescription("Inception").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.StateDescription,
                "2010, IMDb 8.8, Rotten Tomatoes 87 percent",
            ),
        )
        ratings = listOf("IMDb 8.8")
        compose.waitForIdle()
        assertEquals(0, count("RT 87%"))
        // The phone's Home posters are the art alone: no name, no year, no scores.
        captioned = false
        compose.waitForIdle()
        assertEquals(0, count("IMDb 8.8"))
        assertEquals(0, count("2010"))
    }
}
