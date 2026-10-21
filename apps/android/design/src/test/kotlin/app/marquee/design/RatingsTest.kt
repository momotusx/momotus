package app.marquee.design

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** IMDb and Rotten Tomatoes side by side: the meta line and the poster chip show each score there is, and nothing else. */
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
    fun `a poster chip shows the scores there are and no chip without any`() {
        var ratings by mutableStateOf(listOf("IMDb 8.8", "RT 87%"))
        compose.setContent { MarqueeTheme(Form.Phone) { PosterTile("Inception", null, {}, ratings = ratings) } }
        assertEquals(1, count("IMDb 8.8"))
        assertEquals(1, count("RT 87%"))
        ratings = listOf("IMDb 8.8")
        compose.waitForIdle()
        assertEquals(1, count("IMDb 8.8"))
        assertEquals(0, count("RT 87%"))
        ratings = emptyList()
        compose.waitForIdle()
        assertEquals(0, count("IMDb 8.8"))
    }
}
