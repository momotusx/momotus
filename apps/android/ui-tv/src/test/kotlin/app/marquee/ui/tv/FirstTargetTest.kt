package app.marquee.ui.tv

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

    @Test
    fun `the first tile of the first lane wins`() {
        val rows = listOf(row("a", RowState.Ready, "x", "y"), row("b", RowState.Ready, "z"))
        assertEquals("tile:a:x", firstTarget(rows, patient = true))
    }

    @Test
    fun `a lane still loading holds the focus back instead of handing it to a lower lane`() {
        val rows = listOf(row("a", RowState.Loading), row("b", RowState.Ready, "z"))
        assertNull(firstTarget(rows, patient = true))
        assertEquals("tile:b:z", firstTarget(rows, patient = false))
    }

    @Test
    fun `lanes that came back empty or failed are skipped`() {
        val rows =
            listOf(
                row("a", RowState.Empty),
                row("b", RowState.Failed("timed out")),
                row("c", RowState.Ready, "z"),
            )
        assertEquals("tile:c:z", firstTarget(rows, patient = true))
    }

    @Test
    fun `no tiles anywhere means no target`() {
        assertNull(firstTarget(listOf(row("a", RowState.Empty)), patient = true))
        assertNull(firstTarget(emptyList(), patient = false))
    }
}
