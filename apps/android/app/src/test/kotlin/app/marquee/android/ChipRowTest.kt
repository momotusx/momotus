package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** Two addons can each have a "Popular" catalog of one kind (Cinemeta and The Movie Database Addon). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class ChipRowTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `two catalogs with one name are two chips, each choosing its own`() {
        var picked by mutableStateOf("cinemeta|movie|top")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                val choices = remember { listOf("cinemeta|movie|top" to "Popular", "tmdb|movie|tmdb.top" to "Popular") }
                ChipRow("Catalog", choices, picked) { picked = it }
            }
        }
        val chips = compose.onAllNodesWithText("Popular")
        chips.assertCountEquals(2)
        chips[1].performClick()
        compose.runOnIdle { assertEquals("tmdb|movie|tmdb.top", picked) }
    }
}
