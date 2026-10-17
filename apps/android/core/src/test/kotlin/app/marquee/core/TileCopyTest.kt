package app.marquee.core

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
    fun `a tile with no progress has no caption`() {
        assertNull(TileCopy.caption(tile()))
    }

    @Test
    fun `Continue Watching says where you are and how much is watched`() {
        assertEquals("Movie, 25% watched", TileCopy.caption(tile(progress = 0.25f)))
        assertEquals(
            "S2:E3, 37% watched",
            TileCopy.caption(tile(kind = "series", progress = 0.37f, videoId = "tt1:2:3")),
        )
        assertEquals("50% watched", TileCopy.caption(tile(kind = "channel", progress = 0.5f)))
    }

    @Test
    fun `an Up Next tile keeps the core's own words`() {
        assertEquals("Next: S1 E2", TileCopy.caption(tile(kind = "series", releaseInfo = "Next: S1 E2")))
    }

    @Test
    fun `the source label names the library for Continue Watching`() {
        assertEquals("From your library", TileCopy.source(TileCopy.CONTINUE_ROW, "Local"))
        assertEquals("Cinemeta", TileCopy.source("a:movie:top", "Cinemeta"))
    }

    @Test
    fun `the hero line gives kind and year`() {
        assertEquals("Movie · 2026", TileCopy.heroMeta(tile(releaseInfo = "2026")))
        assertEquals("Series", TileCopy.heroMeta(tile(kind = "series")))
        assertEquals("Podcast", TileCopy.kind("podcast"))
    }
}
