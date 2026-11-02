package app.marquee.ui.tv

import app.marquee.core.TileCopy
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowState
import kotlin.test.Test
import kotlin.test.assertEquals

/** Home's page items: the hero, the lanes, and the one-time card under Continue Watching. */
class PageIndexTest {
    private fun row(id: String) =
        HomeRow(rowId = id, title = id, source = "s", state = RowState.Ready, tiles = emptyList())

    private val withContinue = listOf(row(TileCopy.CONTINUE_ROW), row("popular"), row("new"))
    private val noContinue = listOf(row("popular"), row("new"))

    @Test
    fun `lanes under the card sit one item lower, lanes above it do not`() {
        // hero, Continue Watching, card, popular, new
        assertEquals(
            listOf(1, 3, 4),
            withContinue.indices.map { pageIndex(withContinue, it, hasHero = true, card = true) },
        )
        // hero, card, popular, new: no Continue Watching puts the card first
        assertEquals(listOf(2, 3), noContinue.indices.map { pageIndex(noContinue, it, hasHero = true, card = true) })
        // No card: as before.
        assertEquals(
            listOf(0, 1, 2),
            withContinue.indices.map { pageIndex(withContinue, it, hasHero = false, card = false) },
        )
    }

    @Test
    fun `an alert above the hero puts the hero and every lane one item lower`() {
        // alert, hero, Continue Watching, card, popular, new
        assertEquals(
            listOf(2, 4, 5),
            withContinue.indices.map { pageIndex(withContinue, it, hasHero = true, card = true, alert = true) },
        )
        // alert, popular, new: no hero
        assertEquals(
            listOf(1, 2),
            noContinue.indices.map {
                pageIndex(noContinue, it, hasHero = false, card = false, alert = true)
            },
        )
    }
}
