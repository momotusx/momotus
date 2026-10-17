package app.marquee.ui.tv

import app.marquee.core.TileCopy
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class FirstTargetTest {
    private fun row(
        id: String,
        state: RowState,
        vararg tiles: String,
    ) = HomeRow(
        rowId = id,
        title = id,
        source = "s",
        state = state,
        tiles = tiles.map { Tile(id = it, kind = "movie", name = it, posterShape = "poster") },
    )

    private val movieHero = Hero(Tile(id = "h", kind = "movie", name = "H", posterShape = "poster"), "reason")
    private val seriesHero = Hero(Tile(id = "s", kind = "series", name = "S", posterShape = "poster"), "reason")

    @Test
    fun `the first tile of the first lane wins when there is no hero`() {
        val rows = listOf(row("a", RowState.Ready, "x", "y"), row("b", RowState.Ready, "z"))
        assertEquals("tile:a:x", firstTarget(rows, hero = null, patient = true))
    }

    @Test
    fun `a lane still loading holds the focus back instead of handing it to a lower lane`() {
        val rows = listOf(row("a", RowState.Loading), row("b", RowState.Ready, "z"))
        assertNull(firstTarget(rows, hero = null, patient = true))
        assertEquals("tile:b:z", firstTarget(rows, hero = null, patient = false))
    }

    @Test
    fun `lanes that came back empty or failed are skipped`() {
        val rows =
            listOf(
                row("a", RowState.Empty),
                row("b", RowState.Failed("timed out")),
                row("c", RowState.Ready, "z"),
            )
        assertEquals("tile:c:z", firstTarget(rows, hero = null, patient = true))
    }

    @Test
    fun `no tiles anywhere means no target`() {
        assertNull(firstTarget(listOf(row("a", RowState.Empty)), hero = null, patient = true))
        assertNull(firstTarget(emptyList(), hero = null, patient = false))
    }

    @Test
    fun `Continue Watching leads, otherwise the hero's Play does`() {
        val withContinue = listOf(row(TileCopy.CONTINUE_ROW, RowState.Ready, "c"), row("b", RowState.Ready, "z"))
        assertEquals("tile:continue-watching:c", firstTarget(withContinue, movieHero, patient = true))
        val without = listOf(row("b", RowState.Ready, "z"))
        assertEquals("hero:play", firstTarget(without, movieHero, patient = true))
    }

    @Test
    fun `a series hero has no single video to play, so its Details takes the focus`() {
        assertEquals("hero:details", firstTarget(listOf(row("b", RowState.Ready, "z")), seriesHero, patient = true))
    }
}
