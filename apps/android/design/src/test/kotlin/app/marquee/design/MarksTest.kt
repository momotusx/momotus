package app.marquee.design

import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class MarksTest {
    private fun mark(name: String) = File("../../../design/marks/$name.svg").readText()

    /** The drawn path data is design/marks' own, unaltered. */
    @Test
    fun `the marks draw design marks paths`() {
        Marks.IMDB.drop(1).forEach { assertTrue(it.take(40), mark("imdb").contains("d=\"$it\"")) }
        assertTrue(mark("imdb").contains("points=\"0 18 5 18 5 0 0 0\""))
        assertTrue(mark("tmdb").contains("d=\"${Marks.TMDB}\""))
        Marks.TRAKT.forEach { assertTrue(it.take(40), mark("trakt").contains("d=\"$it\"")) }
        assertTrue(mark("trakt").contains("fill=\"#6438D0\""))
    }
}
