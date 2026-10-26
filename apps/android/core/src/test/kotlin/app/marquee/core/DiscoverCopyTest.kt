package app.marquee.core

import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.DiscoverState
import app.marquee.core.model.Platform
import app.marquee.core.model.Ratings
import app.marquee.core.model.Scope
import app.marquee.core.model.SortChoice
import app.marquee.core.model.Tile
import kotlin.test.Test
import kotlin.test.assertEquals

/** Discover's footer, empty line and caption, as the web words them (apps/web/src/screens/discover.tsx). */
class DiscoverCopyTest {
    private fun snapshot(
        scope: Scope = Scope(services = 0, failed = 0, pending = 0),
        genre: String? = null,
        decade: String? = null,
        platform: String? = null,
    ) = DiscoverSnapshot(
        genreRequired = false,
        platforms = listOf(Platform(id = "netflix", name = "Netflix")),
        platform = platform,
        sorts = listOf(SortChoice(id = "imdb", label = "IMDb rating", mark = "imdb")),
        sort = "imdb",
        genre = genre,
        decade = decade,
        scope = scope,
        state = DiscoverState.End,
    )

    @Test
    fun `the footer says the size, the scope beside the sort and what did not answer`() {
        val s = snapshot(Scope(titles = 857, services = 10, failed = 2, pending = 1, note = "on your services"))
        assertEquals(
            "857 titles · IMDb rating, on your services · 1 service still loading · 2 services did not answer",
            DiscoverCopy.scope(s),
        )
    }

    @Test
    fun `the empty line names what was asked`() {
        assertEquals(
            "No Drama on Netflix right now.",
            DiscoverCopy.empty(snapshot(genre = "Drama", platform = "netflix")),
        )
        assertEquals("No titles from the 1990s right now.", DiscoverCopy.empty(snapshot(decade = "1990s")))
    }

    @Test
    fun `the corner shows the value the list is ordered by`() {
        val tile =
            Tile(
                id = "tt1",
                kind = "movie",
                name = "One",
                posterShape = "poster",
                releaseInfo = "2021",
                ratings = Ratings(imdb = "8.4"),
            )
        assertEquals("IMDb 8.4", DiscoverCopy.corner("imdb", tile))
        assertEquals("2021", DiscoverCopy.corner("newest", tile))
        assertEquals(null, DiscoverCopy.corner("popular", tile))
        // The caption does not repeat the year the corner shows.
        assertEquals(null, DiscoverCopy.caption("newest", tile))
        assertEquals("2021", DiscoverCopy.caption("imdb", tile))
    }
}
