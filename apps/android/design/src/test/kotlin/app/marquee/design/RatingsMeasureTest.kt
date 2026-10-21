package app.marquee.design

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.text.TextLayoutResult
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The widest scores there are ("IMDb 10.0", "RT 100%") measured with real text layout: on a poster
 * they fit inside the art (stacked on a phone, side by side on TV), and on the meta line each stays
 * one whole line. The widths print as `ratings-measure` lines for bench/core/ratings.jsonl.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp")
class RatingsMeasureTest {
    @get:Rule
    val compose = createComposeRule()

    private val widest = listOf("IMDb 10.0", "RT 100%")

    private fun layout(node: SemanticsNode): TextLayoutResult {
        val results = mutableListOf<TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)
        return results.single()
    }

    private fun score(text: String) = compose.onNodeWithText(text, useUnmergedTree = true).fetchSemanticsNode()

    private fun dp(px: Float) = px / compose.density.density

    private fun whole(text: String) {
        val result = layout(score(text))
        assertEquals(1, result.lineCount, "$text breaks")
        // Laid out on one line whatever the width, so the box must hold the whole line.
        val needs = result.multiParagraph.maxIntrinsicWidth
        assertTrue(result.size.width + 0.5f >= needs, "$text is cut: ${result.size.width} of $needs px")
    }

    private fun poster(
        form: Form,
        ratings: List<String> = widest,
        stacked: Boolean,
    ) {
        compose.setContent { MarqueeTheme(form) { PosterTile("Widest", null, {}, ratings = ratings) } }
        val art = compose.onNodeWithContentDescription("Widest").fetchSemanticsNode().boundsInRoot
        val chip = compose.onNodeWithTag("rating-chip", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        ratings.forEach(::whole)
        assertTrue(chip.left >= art.left && chip.right <= art.right, "$form: the chip leaves the poster")
        val (imdb, rt) = ratings.map { score(it).boundsInRoot }
        if (stacked) {
            assertTrue(rt.top >= imdb.bottom - 0.5f, "$form: the scores are not stacked")
        } else {
            assertEquals(imdb.center.y, rt.center.y, 0.5f, "$form: the scores are not side by side")
        }
        println(
            "ratings-measure {\"platform\":\"android\",\"form\":\"$form\",\"where\":\"poster chip\"," +
                "\"scores\":\"${ratings.joinToString(" ")}\"," +
                "\"chip\":${dp(chip.width)},\"poster\":${dp(art.width)},\"parts\":[${dp(imdb.width)},${dp(rt.width)}]}",
        )
    }

    private fun line(form: Form) {
        compose.setContent {
            MarqueeTheme(form) { RatingLine(widest, "2011–2019 · 148 min · Drama, Science Fiction", onFirst = {}) }
        }
        widest.forEach(::whole)
        val (imdb, rt) = widest.map { score(it).boundsInRoot }
        assertEquals(imdb.center.y, rt.center.y, 0.5f, "$form: the scores are not side by side")
        println(
            "ratings-measure {\"platform\":\"android\",\"form\":\"$form\",\"where\":\"meta line\",\"parts\":[${dp(
                imdb.width,
            )},${dp(rt.width)}]}",
        )
    }

    @Test
    fun `on a phone poster the widest scores stack and fit`() = poster(Form.Phone, stacked = true)

    @Test
    fun `on a TV poster the widest pair goes on two lines and fits`() = poster(Form.Tv, stacked = true)

    @Test
    fun `on a TV poster a usual pair sits side by side and fits`() =
        poster(Form.Tv, listOf("IMDb 8.8", "RT 87%"), stacked = false)

    @Test
    fun `the phone meta line keeps both scores whole`() = line(Form.Phone)

    @Test
    fun `the TV meta line keeps both scores whole`() = line(Form.Tv)
}
