package app.marquee.ui.tv

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class DpadThrottleTest {
    @Test
    fun `a first press always goes through, however soon after the last`() {
        val throttle = DpadThrottle()
        assertTrue(throttle.allow(Axis.Horizontal, repeat = false, nowMs = 1_000))
        assertTrue(throttle.allow(Axis.Horizontal, repeat = false, nowMs = 1_005))
    }

    @Test
    fun `a held key passes at most once per gap on each axis`() {
        val throttle = DpadThrottle(horizontalMs = 80, verticalMs = 112)
        assertTrue(throttle.allow(Axis.Horizontal, repeat = false, nowMs = 0))
        assertFalse(throttle.allow(Axis.Horizontal, repeat = true, nowMs = 30))
        assertFalse(throttle.allow(Axis.Horizontal, repeat = true, nowMs = 79))
        assertTrue(throttle.allow(Axis.Horizontal, repeat = true, nowMs = 80))
        assertFalse(throttle.allow(Axis.Horizontal, repeat = true, nowMs = 120))

        assertTrue(throttle.allow(Axis.Vertical, repeat = false, nowMs = 1_000))
        assertFalse(throttle.allow(Axis.Vertical, repeat = true, nowMs = 1_111))
        assertTrue(throttle.allow(Axis.Vertical, repeat = true, nowMs = 1_112))
    }

    @Test
    fun `a dropped repeat does not push the next one further out`() {
        val throttle = DpadThrottle(horizontalMs = 80, verticalMs = 112)
        throttle.allow(Axis.Horizontal, repeat = false, nowMs = 0)
        val passed = (10L..400L step 10L).filter { throttle.allow(Axis.Horizontal, repeat = true, nowMs = it) }
        // Key events every 10 ms: one passes every 80 ms.
        assertEquals(listOf(80L, 160L, 240L, 320L, 400L), passed)
    }

    @Test
    fun `the two axes do not throttle each other`() {
        val throttle = DpadThrottle()
        throttle.allow(Axis.Horizontal, repeat = false, nowMs = 0)
        assertTrue(throttle.allow(Axis.Vertical, repeat = true, nowMs = 10))
    }
}
