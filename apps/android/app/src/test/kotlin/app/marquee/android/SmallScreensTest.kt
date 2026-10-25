package app.marquee.android

import android.app.Application
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.SoftwareKeyboardController
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** Small things of the journey review (P3-1, P3-2, P3-4). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SmallScreensTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a page that could not open offers Try again, and it asks again`() {
        var retries = 0
        compose.setContent {
            MarqueeTheme(
                Form.Phone,
            ) { Note("Library could not open: timed out", failed = true, onRetry = { retries++ }) }
        }
        compose.onNodeWithText("Try again").performClick()
        compose.runOnIdle { assertEquals(1, retries) }
    }

    @Test
    fun `the keyboard's Search key puts the keyboard away`() {
        var hidden = 0
        val keyboard =
            object : SoftwareKeyboardController {
                override fun show() = Unit

                override fun hide() {
                    hidden++
                }
            }
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalSoftwareKeyboardController provides keyboard) { SearchField("sintel") {} }
            }
        }
        compose.onNodeWithContentDescription("Search titles").performImeAction()
        compose.runOnIdle { assertEquals(1, hidden) }
    }

    @Test
    fun `the start says it is starting after half a second, never at once`() {
        compose.setContent { MarqueeTheme(Form.Tv) { StartingScreen() } }
        compose.onAllNodesWithText(STARTING).assertCountEquals(0)
        // Half a second of real time: the wait is not on the test clock.
        compose.waitUntil(5_000) { compose.onAllNodesWithText(STARTING).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Momotus").assertExists()
    }
}
