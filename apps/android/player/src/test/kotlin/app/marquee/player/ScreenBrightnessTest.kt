package app.marquee.player

import kotlin.test.Test
import kotlin.test.assertEquals

/** The swipe's brightness level against the window's value (plans/player-gestures.md 3.3, tests 4 and 7). */
class ScreenBrightnessTest {
    @Test
    fun `the window value follows a perceptual curve with a floor`() {
        assertEquals(0.01f, brightnessToWindow(0f))
        assertEquals(1f, brightnessToWindow(1f))
        // Half the swipe is about a fifth of the panel's light, as the system's own slider.
        assertEquals(0.218f, brightnessToWindow(0.5f), 0.001f)
        assertEquals(0.5f, windowToLevel(brightnessToWindow(0.5f)), 1e-4f)
    }

    @Test
    fun `a swipe starts from the held level, not from the floor read back`() {
        // The window holds 0.01 at the bottom; read back it would be a 12 percent start.
        assertEquals(0f, startLevel(0f, 255))
        assertEquals(windowToLevel(128 / 255f), startLevel(null, 128), 1e-6f)
    }
}
