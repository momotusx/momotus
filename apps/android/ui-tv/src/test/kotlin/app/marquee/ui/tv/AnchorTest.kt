package app.marquee.ui.tv

import kotlin.test.Test
import kotlin.test.assertEquals

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
}
