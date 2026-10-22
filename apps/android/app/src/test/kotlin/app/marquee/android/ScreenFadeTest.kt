package app.marquee.android

import org.junit.Test
import java.io.File
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The fade between screens leaves them unclipped: AnimatedContent clips to its box while a clipping size
 * transform runs, and on TV that cut Home's top shelf art off at the tab bar until the fade ended.
 */
class ScreenFadeTest {
    @Test
    fun `the fade between screens never clips them`() {
        assertEquals(false, screenFade(300).sizeTransform?.clip)
    }

    @Test
    fun `every screen change uses that fade`() {
        val root = File("src/main/kotlin/app/marquee/android/AppRoot.kt").readText()
        assertTrue("val fade = screenFade(Marquee.panelMs)" in root)
        for (spec in listOf("transitionSpec", "popTransitionSpec", "predictivePopTransitionSpec")) {
            assertTrue(Regex("""\b$spec = \{ fade \}""").containsMatchIn(root), "$spec is not the fade")
        }
        // togetherWith brings back the clipping size transform.
        assertFalse("togetherWith" in root)
    }
}
