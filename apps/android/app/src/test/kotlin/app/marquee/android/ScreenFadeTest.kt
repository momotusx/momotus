package app.marquee.android

import org.junit.Test
import kotlin.test.assertEquals

/**
 * The fade between screens leaves them unclipped: AnimatedContent clips to its box while a clipping size
 * transform runs, and on TV that cut Home's top shelf art off at the tab bar until the fade ended.
 */
class ScreenFadeTest {
    @Test
    fun `the fade between screens never clips them`() {
        assertEquals(false, screenFade(300).sizeTransform?.clip)
    }
}
