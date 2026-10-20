package app.marquee.design

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.Density
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The TV canvas honors the system text size, up to 1.3 (journey review P3-10). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TvCanvasTest {
    @get:Rule
    val compose = createComposeRule()

    private fun inside(system: Float): Float {
        var seen = 0f
        compose.setContent {
            CompositionLocalProvider(LocalDensity provides Density(2f, fontScale = system)) {
                TvCanvas { seen = LocalDensity.current.fontScale }
            }
        }
        compose.waitForIdle()
        return seen
    }

    @Test
    fun `a larger system text size holds on the TV`() {
        assertEquals(1.2f, inside(1.2f))
    }

    @Test
    fun `a very large one stops at 1_3`() {
        assertEquals(TV_MAX_FONT_SCALE, inside(2f))
    }
}
