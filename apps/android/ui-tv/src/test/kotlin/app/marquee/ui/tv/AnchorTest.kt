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
    // A 1000 px wide lane, tiles 240 wide, content starts 34 px in, the next tile should show 140 px.
    private fun distance(offset: Float) = scrollDistance(offset, 240f, 1000f, lead = 34f, trail = 140f)

    @Test
    fun `a tile that fits with the next one peeking stays put`() {
        assertEquals(0f, distance(34f))
        assertEquals(0f, distance(300f))
        assertEquals(0f, distance(620f)) // ends at 860, so 140 px of the next one show
    }

    @Test
    fun `a tile that would hide the next one scrolls to the leading anchor`() {
        assertEquals(587f, distance(621f)) // one pixel too far right: its leading edge goes to 34
        assertEquals(786f, distance(820f))
    }

    @Test
    fun `a tile left of the anchor scrolls back to it`() {
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
    fun `a scroll once started goes on to the anchor, though the child comes into view halfway`() {
        val list = List()
        val spec = AnchoredSpec(lead = 34f, trail = 0f, list = list)
        // A row under the foot: the page starts to bring it to the anchor.
        assertEquals(866f, spec.calculateScrollDistance(900f, 240f, 1000f))
        list.moving = true
        // Compose asks again each frame: in view now, and still on its way to the anchor, not stopped at speed.
        assertEquals(466f, spec.calculateScrollDistance(500f, 240f, 1000f))
        assertEquals(0f, spec.calculateScrollDistance(34f, 240f, 1000f))
        // Still again: a row in view stays put.
        list.moving = false
        assertEquals(0f, spec.calculateScrollDistance(500f, 240f, 1000f))
    }

    @Test
    fun `the anchor leaves the first row's move to the page`() {
        val spec = AnchoredSpec(lead = 34f, trail = 0f, list = List(), hold = { true })
        assertEquals(0f, spec.calculateScrollDistance(-400f, 240f, 1000f))
    }

    @Test
    fun `a lane's move shrinks by what the lane moved, so it lands instead of stopping at speed`() {
        val spec = RoomSpec(room = 34f)
        // A tile past the right edge comes in with the least movement and room for its lift.
        assertEquals(174f, spec.calculateScrollDistance(900f, 240f, 1000f))
        assertEquals(74f, spec.calculateScrollDistance(800f, 240f, 1000f))
        assertEquals(0f, spec.calculateScrollDistance(726f, 240f, 1000f))
        // And one past the left edge.
        assertEquals(-134f, spec.calculateScrollDistance(-100f, 240f, 1000f))
    }
}
