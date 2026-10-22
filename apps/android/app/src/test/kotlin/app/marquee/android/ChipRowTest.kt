package app.marquee.android

import android.app.Application
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.TextLayoutResult
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Two addons can each have a "Popular" catalog of one kind (Cinemeta and The Movie Database Addon). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class ChipRowTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `two catalogs with one name are two chips, each choosing its own`() {
        var picked by mutableStateOf("cinemeta|movie|top")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                val choices = remember { listOf("cinemeta|movie|top" to "Popular", "tmdb|movie|tmdb.top" to "Popular") }
                ChipRow("Catalog", choices, picked) { picked = it }
            }
        }
        val chips = compose.onAllNodesWithText("Popular")
        chips.assertCountEquals(2)
        chips[1].performClick()
        compose.runOnIdle { assertEquals("tmdb|movie|tmdb.top", picked) }
    }

    @Test
    fun `a catalog chip names its addon after its own name`() {
        val sources = mapOf("cinemeta|movie|top" to "Cinemeta", "tmdb|movie|tmdb.top" to "The Movie Database Addon")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                val choices = listOf("cinemeta|movie|top" to "Popular", "tmdb|movie|tmdb.top" to "Popular")
                ChipRow("Catalog", choices, "cinemeta|movie|top", detail = { sources[it] }) {}
            }
        }
        compose.onNodeWithText("Cinemeta").assertExists()
        compose.onNodeWithText("The Movie Database Addon").assertExists()
    }

    @Test
    fun `on TV up to four choices are segments, and more are one pull-down that names the chosen one`() {
        val kinds = listOf("movie" to "Movies", "series" to "Series")
        val genres = (1..6).map { "g$it" to "Genre $it" }
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Column {
                    ChipRow("Type", kinds, "movie") {}
                    ChipRow("Genre", genres, "g3") {}
                }
            }
        }
        compose.onNodeWithText("Movies").assertExists()
        compose.onNodeWithText("Series").assertExists()
        compose.onNodeWithContentDescription("Genre, Genre 3").assertExists()
        compose.onAllNodesWithText("Genre 1").assertCountEquals(0)
    }

    @Test
    @Config(qualifiers = "w360dp-h800dp")
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `on a 360 dp phone three catalogs with their addon scroll in the track, none of their words cut`() {
        // Cinemeta's movie catalogs, each with its addon: about 466 dp of track on a 320 dp page.
        val catalogs = listOf("top" to "Popular", "year" to "New", "imdbRating" to "Featured")
        compose.setContent {
            MarqueeTheme(Form.Phone) { ChipRow("Catalog", catalogs, "top", detail = { "Cinemeta" }) {} }
        }
        val words = catalogs.map { it.second } + "Cinemeta"
        for (word in words) {
            compose.onAllNodesWithText(word, useUnmergedTree = true).fetchSemanticsNodes().forEachIndexed { i, _ ->
                val layout = mutableListOf<TextLayoutResult>()
                compose
                    .onAllNodesWithText(word, useUnmergedTree = true)[i]
                    .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layout) }
                // Laid out whole: as wide as its words, not squeezed to what is left of the row.
                val r = layout.single()
                assertTrue(
                    r.size.width >=
                        r.multiParagraph.intrinsics.maxIntrinsicWidth
                            .toInt(),
                    "$word is cut",
                )
            }
        }
    }
}
