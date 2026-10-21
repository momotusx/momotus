package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.Ratings
import app.marquee.core.model.Tile
import app.marquee.core.model.withoutRemoved
import kotlin.test.Test
import kotlin.test.assertEquals
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
    fun `other rows have no caption, their release info is the year`() {
        assertNull(TileCopy.caption("a:movie:top", tile(releaseInfo = "2010")))
        assertNull(TileCopy.caption(TileCopy.CONTINUE_ROW, tile()))
    }

    @Test
    fun `the source label names the addon, and Continue Watching has none`() {
        assertNull(TileCopy.source(TileCopy.CONTINUE_ROW, "Local"))
        assertEquals("Cinemeta", TileCopy.source("a:movie:top", "Cinemeta"))
    }

    @Test
    fun `a hero says the year, runtime and genres, or the kind`() {
        val full = Hero(tile(releaseInfo = "2026"), "r", runtime = "145 min", genres = listOf("Action", "Adventure"))
        assertEquals("2026 · 145 min · Action, Adventure", TileCopy.heroMeta(full))
        assertEquals("2026", TileCopy.heroMeta(Hero(tile(releaseInfo = "2026"), "r")))
        assertEquals("Series", TileCopy.heroMeta(Hero(tile(kind = "series"), "r")))
    }

    @Test
    fun `only Continue Watching tiles (Next Up among them) offer Remove on a hold`() {
        assertEquals(true, TileCopy.removable(TileCopy.CONTINUE_ROW))
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
    fun `scores sit side by side as plain text and a missing one has no slot`() {
        assertEquals(listOf("IMDb 8.8", "RT 87%"), TileCopy.ratings(Ratings(imdb = "8.8", rottenTomatoes = "87%")))
        assertEquals(listOf("IMDb 8.8"), TileCopy.ratings(Ratings(imdb = "8.8")))
        assertEquals(listOf("RT 0%"), TileCopy.ratings(Ratings(rottenTomatoes = "0%")))
        // The Rotten Tomatoes addon's mark: a word, shown as the core wrote it.
        assertEquals(listOf("IMDb 8.8", "RT Fresh"), TileCopy.ratings(Ratings(imdb = "8.8", rottenTomatoes = "Fresh")))
        assertEquals(emptyList(), TileCopy.ratings(Ratings()))
        // A recording from before the field decodes to no scores, never a placeholder.
        assertEquals(emptyList(), TileCopy.ratings(tile().ratings))
    }
}
