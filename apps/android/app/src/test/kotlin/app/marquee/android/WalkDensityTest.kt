package app.marquee.android

import androidx.compose.ui.unit.Density
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame

/** A walk's narrow screen and large text, made in the app (the device's settings are never touched). */
class WalkDensityTest {
    private val base = Density(3.5f, 1f)

    @Test
    fun `a walk lays the app out at the width and font scale it asks for`() {
        val asked = walkDensity(base, 1344, 360, 1.3f)
        assertEquals(1344 / 360f, asked.density)
        assertEquals(1.3f, asked.fontScale)
        assertEquals(360f, 1344 / asked.density)
    }

    @Test
    fun `without the extras the device's own density stays`() {
        assertSame(base, walkDensity(base, 1344, null, null))
        assertSame(base, walkDensity(base, 1344, 0, 0f))
        assertEquals(1.3f, walkDensity(base, 1344, null, 1.3f).fontScale)
    }
}
