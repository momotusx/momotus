package app.marquee.player

import androidx.compose.ui.unit.IntRect
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** The loading screen's arithmetic (plans/player-polish.md 1.2): the reveal's edge, the buffer fill, the lines, the trim. */
class LoadingFillTest {
    @Test
    fun `the solid copy shows nothing at 0, all of it at 1, with a 6 percent feather between`() {
        val (from0, to0) = revealEdge(0f)
        assertEquals(0f, to0, 1e-6f, "at 0 the feather ends at the left edge: nothing shows")
        assertEquals(-0.06f, from0, 1e-6f)
        val (from1, _) = revealEdge(1f)
        assertEquals(1f, from1, 1e-6f, "at 1 the solid part reaches the right edge")
        val (from, to) = revealEdge(0.4f)
        assertEquals(0.364f, from, 1e-6f)
        assertEquals(0.06f, to - from, 1e-6f)
        assertEquals(revealEdge(1f), revealEdge(1.5f), "past 1 it stays full")
    }

    @Test
    fun `once the player has the URL the buffer fills the rest, from where the core's fill got`() {
        assertEquals(0.3f, bufferFill(0.3f, 0, 1_000), 1e-6f)
        assertEquals(0.65f, bufferFill(0.3f, 500, 1_000), 1e-6f)
        assertEquals(1f, bufferFill(0.3f, 1_000, 1_000), 1e-6f)
        assertEquals(1f, bufferFill(0.3f, 9_000, 1_000), 1e-6f, "never past full")
        assertEquals(0.3f, bufferFill(0.3f, -200, 1_000), 1e-6f, "a playhead past the buffer counts as none")
        assertEquals(0.5f, bufferFill(0f, 1_500, 3_000), 1e-6f, "a stall from nothing, to the rebuffer threshold")
    }

    @Test
    fun `under the mark, what the player says now, then the core's words on a long wait`() {
        assertEquals(
            listOf("Starting 4K, 15.8 GB from Torrentio"),
            loadingLines("Starting 4K, 15.8 GB from Torrentio", null),
        )
        assertEquals(
            listOf("Starting 4K, 15.8 GB from Torrentio", "Finding peers, 3 so far"),
            loadingLines("Starting 4K, 15.8 GB from Torrentio", "Finding peers, 3 so far"),
        )
        assertEquals(listOf("Trying source 2 of 3"), loadingLines(null, "Trying source 2 of 3"))
        assertEquals(listOf("Trying again"), loadingLines("Trying again", "Trying again"), "said once")
        assertEquals(emptyList(), loadingLines(null, null))
    }

    @Test
    fun `a logo is trimmed to the pixels it draws`() {
        val w = 8
        val h = 5
        val pixels = IntArray(w * h)
        // Two marks, at (2,1) and (5,3), and a near-clear speck the trim ignores.
        pixels[1 * w + 2] = 0xFF00FF00.toInt()
        pixels[3 * w + 5] = 0x80FFFFFF.toInt()
        pixels[4 * w + 7] = 0x08FFFFFF
        assertEquals(IntRect(2, 1, 6, 4), drawnBox(pixels, w, h))
        assertNull(drawnBox(IntArray(w * h), w, h), "nothing drawn")
    }

    @Test
    fun `while a logo loads the mark is empty, so the name never shows and gives way to it`() {
        assertEquals(TitleMark.Waiting, titleMark("https://images/logo", null, done = false))
        assertEquals(TitleMark.Name, titleMark("https://images/logo", null, done = true), "it did not load")
        assertEquals(TitleMark.Name, titleMark(null, null, done = false), "no logo")
    }
}
