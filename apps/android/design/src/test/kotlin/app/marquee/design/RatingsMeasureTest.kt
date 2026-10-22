package app.marquee.design

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.test.junit4.createComposeRule
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
 * The widest scores there are ("IMDb 10.0", "RT 100%") measured with real text layout: on the meta line each
 * stays one whole line, the Rotten Tomatoes addon's mark ("RT Fresh") too. A tile's cover carries its scores
 * (RPDB's band), so none is measured under it. The widths print as `ratings-measure` lines for
 * bench/core/ratings.jsonl.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp")
class RatingsMeasureTest {
    @get:Rule
    val compose = createComposeRule()

    private val widest = listOf("IMDb 10.0", "RT 100%")
    private val fresh = listOf("IMDb 10.0", "RT Fresh")

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

    private fun line(
        form: Form,
        ratings: List<String> = widest,
    ) {
        compose.setContent {
            MarqueeTheme(form) { RatingLine(ratings, "2011–2019 · 148 min · Drama, Science Fiction", onFirst = {}) }
        }
        ratings.forEach(::whole)
        val (imdb, rt) = ratings.map { score(it).boundsInRoot }
        assertEquals(imdb.center.y, rt.center.y, 0.5f, "$form: the scores are not side by side")
        println(
            "ratings-measure {\"platform\":\"android\",\"form\":\"$form\",\"where\":\"meta line\"," +
                "\"scores\":\"${ratings.joinToString(" ")}\",\"parts\":[${dp(imdb.width)},${dp(rt.width)}]}",
        )
    }

    @Test
    fun `the phone meta line keeps both scores whole`() = line(Form.Phone)

    @Test
    fun `the phone meta line keeps RT Fresh whole`() = line(Form.Phone, fresh)

    @Test
    fun `the TV meta line keeps RT Fresh whole`() = line(Form.Tv, fresh)

    @Test
    fun `the TV meta line keeps both scores whole`() = line(Form.Tv)
}
