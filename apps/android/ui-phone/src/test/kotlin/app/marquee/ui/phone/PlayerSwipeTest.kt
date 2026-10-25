package app.marquee.ui.phone

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The level swipes' rules (plans/player-gestures.md 3, tests 1 to 6). */
class PlayerSwipeTest {
    private val size = Size(1000f, 400f)
    private val bands = EdgeBands(left = 24f, top = 32f, right = 24f, bottom = 32f)

    @Test
    fun `the start point picks the side, and the edges are the system's`() {
        assertEquals(SwipeSide.Brightness, swipeSide(Offset(490f, 200f), size, bands))
        assertEquals(SwipeSide.Volume, swipeSide(Offset(500f, 200f), size, bands))
        assertNull(swipeSide(Offset(20f, 200f), size, bands))
        assertNull(swipeSide(Offset(980f, 200f), size, bands))
        assertNull(swipeSide(Offset(300f, 20f), size, bands))
        assertNull(swipeSide(Offset(700f, 380f), size, bands))
        // A wider back gesture than the floor widens its band.
        assertNull(swipeSide(Offset(30f, 200f), size, bands.copy(left = 40f)))
    }

    @Test
    fun `a touch commits only when it goes up or down twice as far as across`() {
        assertEquals(true, commits(3f, 12f, 12f))
        assertEquals(true, commits(3f, -12f, 12f))
        // Under 2:1 it is still undecided: a later move can commit.
        assertNull(commits(7f, 12f, 12f))
        assertEquals(false, commits(12f, 5f, 12f))
        assertNull(commits(2f, 11f, 12f))
    }

    @Test
    fun `eighty percent of the height sweeps the whole range`() {
        assertEquals(1f, level(0.2f, -0.8f * 400f, 400f))
        assertEquals(0f, level(0.5f, 0.4f * 400f, 400f))
        assertEquals(0.75f, level(0.5f, -0.2f * 400f, 400f), 1e-6f)
    }

    @Test
    fun `a level is the nearest volume step, and a step reads back as its level`() {
        assertEquals(13, volumeIndex(0.5f, 0, 25))
        assertEquals(25, volumeIndex(1f, 0, 25))
        assertEquals(1, volumeIndex(0f, 1, 25))
        assertEquals(8f / 15f, indexLevel(8, 0, 15))
        assertEquals(1f, indexLevel(15, 0, 15))
    }

    @Test
    fun `the tick comes once at either end`() {
        assertTrue(reachedEnd(0.98f, 1f))
        assertFalse(reachedEnd(1f, 1f))
        assertTrue(reachedEnd(0.02f, 0f))
        assertFalse(reachedEnd(0.4f, 0.5f))
    }
}
