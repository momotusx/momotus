package app.marquee.design

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
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
 * The widest scores there are ("IMDb 10.0", "RT 100%") measured with real text layout: under a poster they
 * stay in its column (side by side, or the second on the next line), and on the meta line each stays one
 * whole line. The Rotten Tomatoes addon's mark ("RT Fresh") is held to the same. The widths print
 * as `ratings-measure` lines for bench/core/ratings.jsonl.
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

    private fun poster(
        form: Form,
        ratings: List<String> = widest,
        /** True: one line; false: the pair wraps; null: either. */
        sideBySide: Boolean? = null,
    ) {
        compose.setContent { MarqueeTheme(form) { PosterTile("Widest", null, {}, ratings = ratings) } }
        val art = compose.onNodeWithContentDescription("Widest").fetchSemanticsNode().boundsInRoot
        ratings.forEach(::whole)
        val (imdb, rt) = ratings.map { score(it).boundsInRoot }
        for (part in listOf(imdb, rt)) {
            assertTrue(part.top >= art.bottom, "$form: a score sits on the art")
            assertTrue(
                part.left >= art.left - 0.5f && part.right <= art.right + 0.5f,
                "$form: a score runs past the poster",
            )
        }
        when (sideBySide) {
            true -> assertEquals(imdb.center.y, rt.center.y, 0.5f, "$form: the scores are not side by side")
            false -> assertTrue(rt.top >= imdb.bottom - 0.5f, "$form: the scores did not wrap")
            null -> Unit
        }
        // The width the scores take under the poster, on one line or two.
        val row = dp(maxOf(imdb.right, rt.right) - minOf(imdb.left, rt.left))
        println(
            "ratings-measure {\"platform\":\"android\",\"form\":\"$form\",\"where\":\"poster caption\"," +
                "\"scores\":\"${ratings.joinToString(" ")}\",\"row\":$row," +
                "\"poster\":${dp(art.width)},\"parts\":[${dp(imdb.width)},${dp(rt.width)}]}",
        )
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
    fun `under a phone poster the widest scores wrap and stay in its column`() = poster(Form.Phone, sideBySide = false)

    @Test
    fun `under a TV poster the widest pair stays in its column`() = poster(Form.Tv)

    @Test
    fun `under a TV poster a usual pair sits side by side`() =
        poster(Form.Tv, listOf("IMDb 8.8", "RT 87%"), sideBySide = true)

    // The frames' check (tv.ts:131): at the 23 floor with a 14 gap "IMDb 8.0  RT Fresh" fits the 217 poster.
    @Test
    fun `under a TV poster RT Fresh with a usual IMDb score sits side by side`() =
        poster(Form.Tv, listOf("IMDb 8.8", "RT Fresh"), sideBySide = true)

    @Test
    fun `under a TV poster RT Fresh beside the widest IMDb score stays in its column`() = poster(Form.Tv, fresh)

    @Test
    fun `under a phone poster RT Fresh stays in its column`() = poster(Form.Phone, fresh)

    @Test
    fun `the phone meta line keeps both scores whole`() = line(Form.Phone)

    @Test
    fun `the phone meta line keeps RT Fresh whole`() = line(Form.Phone, fresh)

    @Test
    fun `the TV meta line keeps RT Fresh whole`() = line(Form.Tv, fresh)

    @Test
    fun `the TV meta line keeps both scores whole`() = line(Form.Tv)
}
