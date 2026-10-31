package app.marquee.design

import org.junit.Test
import kotlin.test.assertEquals

/** A tile's caption line ends at a whole word and an ellipsis, never inside a word (audit P2, DATA-10). */
class WordFitTest {
    private fun fit(
        text: String,
        chars: Int,
    ) = wordFit(text) { it.length <= chars }

    @Test
    fun `a line that fits stays whole`() {
        assertEquals("Dune", fit("Dune", 10))
    }

    @Test
    fun `a long line ends at the last whole word that fits`() {
        assertEquals("Game of…", fit("Game of Thrones", 11))
        assertEquals("S2:E3 · 36 min…", fit("S2:E3 · 36 min left", 16))
    }

    @Test
    fun `a separator left at the end goes with the words after it`() {
        assertEquals("S2:E3…", fit("S2:E3 · 36 min left", 8))
    }

    @Test
    fun `a first word too long for the line is left to the line's own cut`() {
        assertEquals("Supercalifragilistic", fit("Supercalifragilistic", 5))
        assertEquals("Supercalifragilistic film", fit("Supercalifragilistic film", 5))
    }
}
