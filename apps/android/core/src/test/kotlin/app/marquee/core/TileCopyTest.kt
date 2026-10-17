package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.Tile
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
    fun `the source label names the library for Continue Watching`() {
        assertEquals("From your library", TileCopy.source(TileCopy.CONTINUE_ROW, "Local"))
        assertEquals("Cinemeta", TileCopy.source("a:movie:top", "Cinemeta"))
    }

    @Test
    fun `a hero says the year, runtime and genres, or the kind`() {
        val full = Hero(tile(releaseInfo = "2026"), "r", runtime = "145 min", genres = listOf("Action", "Adventure"))
        assertEquals("2026 · 145 min · Action, Adventure", TileCopy.heroMeta(full))
        assertEquals("2026", TileCopy.heroMeta(Hero(tile(releaseInfo = "2026"), "r")))
        assertEquals("Series", TileCopy.heroMeta(Hero(tile(kind = "series"), "r")))
    }
}
