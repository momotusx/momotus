package app.marquee.android

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** A title page's genre chip opens Discover, then one select once its first snapshot is in (plans/title-page-ratings-genres-similar.md 2). */
class GenreAskTest {
    @Test
    fun `the select waits for Discover's first snapshot, then goes once`() {
        val ask = DiscoverRequest("movie", "Drama")
        assertNull(genreAsk(ask, opened = false))
        assertEquals(ask, genreAsk(ask, opened = true))
        // Sent: the request is cleared, so nothing goes again.
        assertNull(genreAsk(null, opened = true))
    }
}
