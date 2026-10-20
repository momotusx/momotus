package app.marquee.core

import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.model.allFailed
import app.marquee.core.model.notice
import app.marquee.core.model.tileKeys
import app.marquee.core.route.Route
import app.marquee.core.route.afterChoice
import app.marquee.core.route.isTopLevel
import app.marquee.core.route.restoredStack
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
    fun `only the six places are top level`() {
        assertTrue(Route.Home.isTopLevel)
        assertTrue(Route.Calendar.isTopLevel)
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

    @Test
    fun `Home offers one Try again only when every lane ended with nothing and one failed`() {
        val failed = row(RowState.Failed("offline"))
        assertTrue(listOf(failed, failed).allFailed())
        assertTrue(listOf(failed, row(RowState.Empty)).allFailed())
        assertFalse(listOf(failed, row(RowState.Loading)).allFailed(), "a lane still on its way")
        assertFalse(listOf(failed, row(RowState.Ready, "a")).allFailed(), "a lane with tiles")
        assertFalse(listOf(row(RowState.Failed("late"), "a")).allFailed(), "tiles that arrived before the failure")
        assertFalse(listOf(row(RowState.Empty)).allFailed(), "empty, not failed")
        assertFalse(emptyList<HomeRow>().allFailed())
    }

    @Test
    fun `a stack restored after the process died never brings the player back, its title page stands in`() {
        val player = Route.Player("movie", "tt1", "tt1", "Sintel", key = "k1")
        val versions = Route.Versions("movie", "tt1", "tt1", "Sintel")
        // From the title page: the page is there already.
        assertEquals(listOf(Route.Home, title), restoredStack(listOf(Route.Home, title, player)))
        // From Home's Play, and with Versions opened over the player.
        assertEquals(listOf(Route.Home, title), restoredStack(listOf(Route.Home, player)))
        assertEquals(listOf(Route.Home, title), restoredStack(listOf(Route.Home, title, player, versions)))
        // A title page opened from Calendar keeps its season.
        val calendar = Route.Title("movie", "tt1", season = 2)
        assertEquals(listOf(Route.Home, calendar), restoredStack(listOf(Route.Home, calendar, player)))
        // Without a player, nothing changes.
        assertEquals(listOf(Route.Home, title, versions), restoredStack(listOf(Route.Home, title, versions)))
    }

    @Test
    fun `Versions opened from the player gives way with it to the chosen version, Back from it finds the player`() {
        val playing = Route.Player("movie", "tt1", "tt1", "Sintel", key = "k1")
        val versions = Route.Versions("movie", "tt1", "tt1", "Sintel")
        val chosen = Route.Player("movie", "tt1", "tt1", "Sintel", key = "k2")
        // Over the paused player: both go, the chosen version plays where the player was.
        assertEquals(
            listOf(Route.Home, title, chosen),
            afterChoice(listOf(Route.Home, title, playing, versions), chosen),
        )
        // From the title page: the chosen version plays over Versions, as before.
        assertEquals(
            listOf(Route.Home, title, versions, chosen),
            afterChoice(listOf(Route.Home, title, versions), chosen),
        )
        // A player of another video under it is not the one Versions came from.
        val other = Route.Player("series", "tt2", "tt2:1:1", "Severance")
        assertEquals(
            listOf(Route.Home, other, versions, chosen),
            afterChoice(listOf(Route.Home, other, versions), chosen),
        )
    }
}
