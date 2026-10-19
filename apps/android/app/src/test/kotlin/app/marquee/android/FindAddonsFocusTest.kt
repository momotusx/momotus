package app.marquee.android

import app.marquee.android.FindAddonsFocus.Ask
import app.marquee.android.FindAddonsFocus.Target
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** TV focus on Find addons (plans/m12-addon-discovery.md 3.6) and a card's select. */
class FindAddonsFocusTest {
    private val cards = listOf("a", "b", "c")

    private fun target(
        ask: Ask,
        failed: Boolean = false,
        loading: Boolean = false,
        visible: List<String> = cards,
        lastCard: String? = null,
        waited: Boolean = false,
    ) = FindAddonsFocus.target(ask, failed, loading, visible, lastCard, waited)

    @Test
    fun `the page opens on the List chip, on Try again with no list, on Paste a link while none came`() {
        assertEquals(Target.Chip, target(Ask.Open))
        assertEquals(Target.TryAgain, target(Ask.Open, failed = true, visible = emptyList()))
        // While the list is on its way the landing waits, then takes Paste a link.
        assertNull(target(Ask.Open, loading = true, visible = emptyList()))
        assertEquals(Target.Paste, target(Ask.Open, loading = true, visible = emptyList(), waited = true))
    }

    @Test
    fun `Back from the cards, Refresh and Try again land on the List chip`() {
        assertEquals(Target.Chip, target(Ask.Back, lastCard = "b"))
        assertEquals(Target.Chip, target(Ask.Retry, loading = true))
    }

    @Test
    fun `down into the cards lands on the card left last, else the first`() {
        assertEquals(Target.Card("c"), target(Ask.EnterCards, lastCard = "c"))
        // A filter took the card away meanwhile.
        assertEquals(Target.Card("a"), target(Ask.EnterCards, visible = listOf("a", "b"), lastCard = "c"))
        assertEquals(Target.Card("a"), target(Ask.EnterCards))
        assertNull(target(Ask.EnterCards, visible = emptyList(), lastCard = "c"))
    }

    @Test
    fun `when the focused card goes, focus takes the first card, else the chip`() {
        assertEquals(Target.Card("a"), target(Ask.CardGone, lastCard = "z"))
        assertEquals(Target.Chip, target(Ask.CardGone, visible = emptyList(), lastCard = "z"))
    }

    @Test
    fun `a select acts only on the up of a down that began on the same card`() {
        val gate = SelectGate()
        // The down went to another control (the select that moved focus here).
        assertEquals(false, gate.keyUp())
        gate.keyDown()
        assertEquals(true, gate.keyUp())
        assertEquals(false, gate.keyUp())
        // Focus left between the down and the up.
        gate.keyDown()
        gate.reset()
        assertEquals(false, gate.keyUp())
    }
}
