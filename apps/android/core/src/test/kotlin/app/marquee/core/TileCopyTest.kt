package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.HomeRow
import app.marquee.core.model.Ratings
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.model.withoutRemoved
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull

class TileCopyTest {
    private fun tile(
        kind: String = "movie",
        progress: Float? = null,
        videoId: String? = null,
        releaseInfo: String? = null,
    ) = Tile(
        id = "t",
        kind = kind,
        name = "N",
        posterShape = "poster",
        progress = progress,
        videoId = videoId,
        releaseInfo = releaseInfo,
    )

    @Test
    fun `Continue Watching shows the core's caption as it is`() {
        assertEquals(
            "S2:E3 · 36 min left",
            TileCopy.caption(
                TileCopy.CONTINUE_ROW,
                tile(kind = "series", progress = 0.37f, releaseInfo = "S2:E3 · 36 min left"),
            ),
        )
        assertEquals(
            "36 min left",
            TileCopy.caption(TileCopy.CONTINUE_ROW, tile(progress = 0.25f, releaseInfo = "36 min left")),
        )
        assertEquals(
            "Next: S1:E2",
            TileCopy.caption(TileCopy.CONTINUE_ROW, tile(kind = "series", releaseInfo = "Next: S1:E2")),
        )
    }

    @Test
    fun `a cover is RPDB's rated poster then the addon's, and wide art never shows the ratings band`() {
        val rpdb = "https://api.ratingposterdb.com/k/imdb/poster-default/tt1.jpg"
        val rated =
            tile().copy(
                poster = rpdb,
                posterFallback = "https://addon/p.jpg",
                background = "https://addon/b.jpg",
            )
        assertEquals(rpdb to "https://addon/p.jpg", TileCopy.art(rated, wide = false))
        assertEquals("https://addon/b.jpg" to "https://addon/p.jpg", TileCopy.art(rated, wide = true))
        // No backdrop: the 16:9 card and the hero take the addon's poster, not RPDB's.
        assertEquals("https://addon/p.jpg" to null, TileCopy.art(rated.copy(background = null), wide = true))
        // A poster that is the addon's own already has no fallback.
        val plain = tile().copy(poster = "https://addon/p.jpg")
        assertEquals("https://addon/p.jpg" to null, TileCopy.art(plain, wide = false))
        assertEquals("https://addon/p.jpg" to null, TileCopy.art(plain, wide = true))
    }

    @Test
    fun `other rows have no caption, their release info is the year`() {
        assertNull(TileCopy.caption("a:movie:top", tile(releaseInfo = "2010")))
        assertNull(TileCopy.caption(TileCopy.CONTINUE_ROW, tile()))
    }

    @Test
    fun `Home, Search and Discover never read an addon's name, not even for two rows of one title`() {
        // The owner, 2026-10-22 (design/DECISION.md, Shelves): only the Addons pages and Home rows name it.
        val screens =
            listOf(
                "../ui-tv/src/main/kotlin/app/marquee/ui/tv/TvHome.kt",
                "../ui-phone/src/main/kotlin/app/marquee/ui/phone/PhoneHome.kt",
                "../app/src/main/kotlin/app/marquee/android/BrowseScreens.kt",
                "src/main/kotlin/app/marquee/core/model/HomeRows.kt",
            )
        for (path in screens) {
            val text = File(path).readText()
            assertFalse(
                Regex("""\.source\b|\${'$'}source\b|twinSource""").containsMatchIn(text),
                "$path reads an addon's name",
            )
        }
    }

    @Test
    fun `a Home rows line names the addon only beside a twin title, then says Hidden`() {
        fun catalog(
            id: String,
            title: String,
            source: String,
            hidden: Boolean = false,
        ) = HomeCatalog(id, title, source, hidden)
        val movies = catalog("cinemeta:movie:top", "Popular Movies", "Cinemeta")
        val series = catalog("cinemeta:series:top", "Popular Series", "Cinemeta", hidden = true)
        val tmdb = catalog("tmdb:movie:top", "Popular Movies", "The Movie Database")
        assertEquals(null, TileCopy.homeRowLine(movies, listOf(movies, series), "Hidden"))
        assertEquals("Hidden", TileCopy.homeRowLine(series, listOf(movies, series), "Hidden"))
        assertEquals("The Movie Database", TileCopy.homeRowLine(tmdb, listOf(movies, series, tmdb), "Hidden"))
        val twin = series.copy(title = "Popular Movies")
        assertEquals("Cinemeta · Hidden", TileCopy.homeRowLine(twin, listOf(movies, twin), "Hidden"))
    }

    @Test
    fun `a hero says its kind and genres, then its year and runtime`() {
        val full = Hero(tile(releaseInfo = "2026"), "r", runtime = "145 min", genres = listOf("Action", "Adventure"))
        // The top shelf's two lines: the kind and genres (600), then the facts before the ratings row.
        assertEquals("Movie · Action · Adventure", TileCopy.heroGenres(full))
        assertEquals("2026 · 145 min", TileCopy.heroFacts(full))
        assertNull(TileCopy.heroFacts(Hero(tile(kind = "series"), "r")))
    }

    @Test
    fun `only Continue Watching tiles (Next Up among them) offer Remove on a hold`() {
        assertEquals(true, TileCopy.removable(TileCopy.CONTINUE_ROW))
        assertEquals(true, TileCopy.isContinue(TileCopy.CONTINUE_ROW))
        assertEquals(false, TileCopy.removable("com.linvo.cinemeta:movie:top"))
    }

    @Test
    fun `a removal waiting for Undo leaves its tile out, and an emptied Continue Watching goes`() {
        val cw =
            app.marquee.core.model.HomeRow(
                TileCopy.CONTINUE_ROW,
                "Continue Watching",
                "",
                app.marquee.core.model.RowState.Ready,
                listOf(
                    app.marquee.core.model
                        .Tile(id = "a", kind = "movie", name = "A", posterShape = "poster"),
                    app.marquee.core.model
                        .Tile(id = "b", kind = "movie", name = "B", posterShape = "poster"),
                ),
            )
        val other = cw.copy(rowId = "pop", title = "Popular")
        val rows = listOf(cw, other)
        assertEquals(
            listOf("b"),
            rows
                .withoutRemoved("a")
                .first()
                .tiles
                .map { it.id },
        )
        assertEquals(
            2,
            rows
                .withoutRemoved("a")
                .last()
                .tiles.size,
            "other rows keep the title",
        )
        val alone = listOf(cw.copy(tiles = cw.tiles.take(1)), other)
        assertEquals(listOf("pop"), alone.withoutRemoved("a").map { it.rowId })
        assertEquals(rows, rows.withoutRemoved(null))
    }

    @Test
    fun `text shows IMDb's number alone, as Stremio does, and a missing one has no slot`() {
        // Rotten Tomatoes shows only in the covers' band, never as text: the core sends IMDb alone.
        assertEquals(listOf("IMDb 8.8"), TileCopy.ratings(Ratings(imdb = "8.8")))
        assertEquals(emptyList(), TileCopy.ratings(Ratings()))
        // A recording from before the field decodes to no scores, never a placeholder.
        assertEquals(emptyList(), TileCopy.ratings(tile().ratings))
    }
}
