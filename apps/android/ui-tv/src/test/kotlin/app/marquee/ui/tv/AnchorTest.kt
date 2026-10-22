package app.marquee.ui.tv

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.MutatePriority
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.ScrollableState
import app.marquee.design.RoomSpec
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalFoundationApi::class)
class AnchorTest {
    // A page 1000 px tall, cards 240 tall with 60 px of lines under them, the anchor 34 px down.
    private fun distance(offset: Float) = scrollDistance(offset, 240f, 1000f, lead = 34f, trail = 60f)

    @Test
    fun `a card wholly in view with its lines stays put`() {
        assertEquals(0f, distance(34f))
        assertEquals(0f, distance(300f))
        assertEquals(0f, distance(700f)) // its lines end on the page's foot
    }

    @Test
    fun `a card whose lines would be cut scrolls to the anchor`() {
        assertEquals(667f, distance(701f)) // its lines one pixel past the foot: its top goes to 34
        assertEquals(786f, distance(820f))
    }

    @Test
    fun `a card above the anchor scrolls back to it`() {
        assertEquals(-24f, distance(10f))
        assertEquals(-134f, distance(-100f))
    }

    /** A list that is scrolling while [moving]. */
    private class List : ScrollableState {
        var moving = false

        override val isScrollInProgress get() = moving

        override fun dispatchRawDelta(delta: Float) = 0f

        override suspend fun scroll(
            scrollPriority: MutatePriority,
            block: suspend ScrollScope.() -> Unit,
        ) {}
    }

    @Test
    fun `a scroll once started goes on to the anchor, though the row comes into view halfway`() {
        val list = List()
        var lead = 34f
        val spec = AnchoredSpec(list, lead = { lead })
        // A row under the foot: the page starts to bring it to the anchor.
        assertEquals(866f, spec.calculateScrollDistance(900f, 240f, 1000f))
        list.moving = true
        // Compose asks again each frame: in view now, and still on its way to the anchor, not stopped at speed.
        assertEquals(466f, spec.calculateScrollDistance(500f, 240f, 1000f))
        // The anchor moved (a header reported its height): the same scroll goes on to where it is now.
        lead = 50f
        assertEquals(450f, spec.calculateScrollDistance(500f, 240f, 1000f))
        assertEquals(0f, spec.calculateScrollDistance(50f, 240f, 1000f))
        // Still again: a row in view stays put.
        list.moving = false
        assertEquals(0f, spec.calculateScrollDistance(500f, 240f, 1000f))
    }

    @Test
    fun `the anchor leaves the first row's move to the page`() {
        val spec = AnchoredSpec(List(), lead = { 34f }, hold = { true })
        assertEquals(0f, spec.calculateScrollDistance(-400f, 240f, 1000f))
    }

    @Test
    fun `a lane reveals a tile with the least movement and room for the next to peek, at either end`() {
        // A 1920 px lane, the inset and peek (174) clear of either end.
        val spec = RoomSpec(room = 174f)
        assertEquals(0f, spec.calculateScrollDistance(400f, 240f, 1920f))
        // Past the right edge: it comes in so the next tile shows; the distance shrinks by what the lane
        // moved, so Compose's asking again each frame lands it instead of stopping it at speed.
        assertEquals(274f, spec.calculateScrollDistance(1780f, 240f, 1920f))
        assertEquals(174f, spec.calculateScrollDistance(1680f, 240f, 1920f))
        assertEquals(0f, spec.calculateScrollDistance(1506f, 240f, 1920f))
        // Past the left edge.
        assertEquals(-274f, spec.calculateScrollDistance(-100f, 240f, 1920f))
    }

    @Test
    fun `a child too wide for the room at both ends takes the leading room and holds still`() {
        val spec = RoomSpec(room = 174f)
        assertEquals(0f, spec.calculateScrollDistance(174f, 1700f, 1920f))
        assertEquals(-26f, spec.calculateScrollDistance(148f, 1700f, 1920f))
    }
}
