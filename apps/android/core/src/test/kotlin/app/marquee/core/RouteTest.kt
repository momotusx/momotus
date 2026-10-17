package app.marquee.core

import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.model.notice
import app.marquee.core.model.tileKeys
import app.marquee.core.route.Route
import app.marquee.core.route.isTopLevel
import app.marquee.core.route.switchedTo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RouteTest {
    private val title = Route.Title("movie", "tt1")

    @Test
    fun `choosing a place keeps Home as the root and drops what was above`() {
        assertEquals(listOf(Route.Home, Route.Search), switchedTo(listOf(Route.Home), Route.Search))
        assertEquals(
            listOf(Route.Home, Route.Library),
            switchedTo(listOf(Route.Home, Route.Search, title), Route.Library),
        )
        assertEquals(listOf(Route.Home), switchedTo(listOf(Route.Home, Route.Search, title), Route.Home))
    }

    @Test
    fun `a stack that does not start at Home keeps its own root`() {
        assertEquals(listOf(Route.Onboarding, Route.Search), switchedTo(listOf(Route.Onboarding), Route.Search))
    }

    @Test
    fun `only the five places are top level`() {
        assertTrue(Route.Home.isTopLevel)
        assertTrue(Route.Settings.isTopLevel)
        assertFalse(title.isTopLevel)
        assertFalse(Route.Onboarding.isTopLevel)
    }

    private fun tile(id: String) = Tile(id = id, kind = "movie", name = id, posterShape = "poster")

    private fun row(
        state: RowState,
        vararg ids: String,
    ) = HomeRow(rowId = "r", title = "Popular", source = "Cinemeta", state = state, tiles = ids.map(::tile))

    @Test
    fun `tile keys are stable and repeated ids get a suffix`() {
        assertEquals(
            listOf("tile:r:a", "tile:r:b", "tile:r:a#2", "tile:r:a#3"),
            row(RowState.Ready, "a", "b", "a", "a").tileKeys(),
        )
    }

    @Test
    fun `a row shows skeletons or an explanation only while it has no tiles`() {
        assertEquals(RowNotice.Waiting(slow = false, detail = null), row(RowState.Loading).notice())
        assertEquals(true, (row(RowState.Slow).notice() as RowNotice.Waiting).slow)
        assertIs<RowNotice.Explained>(row(RowState.Empty).notice())
        val failed = row(RowState.Failed("timed out")).notice()
        assertIs<RowNotice.Explained>(failed)
        assertTrue(failed.failed && failed.body.contains("timed out"))
        // Tiles that arrived stay on screen whatever happens after.
        assertNull(row(RowState.Failed("late"), "a").notice())
        assertNull(row(RowState.Loading, "a").notice())
        assertNull(row(RowState.Ready, "a").notice())
    }
}
