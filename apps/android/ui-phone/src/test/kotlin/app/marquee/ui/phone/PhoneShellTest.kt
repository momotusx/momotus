package app.marquee.ui.phone

import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** The phone's tab bar is the iPhone's five places (D6): Calendar is a row in Library, which is lit there. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class PhoneShellTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the bar holds five places, and Library is lit while Calendar shows`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneShell(selected = Route.Calendar, showTabs = true, onSelect = {}) {} }
        }
        compose.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab)).assertCountEquals(5)
        compose.onAllNodesWithContentDescription("Calendar").assertCountEquals(0)
        compose.onNodeWithContentDescription("Library").assertIsSelected()
    }

    private fun linkGoesBack(strip: Boolean) {
        var backs = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = strip, onBack = { backs++ }) {}
            }
        }
        compose.onNodeWithText("Back").assertHasClickAction().performClick()
        compose.runOnIdle { assertEquals(1, backs) }
    }

    @Test
    fun `a page under the status bar shows the back link and it goes back`() = linkGoesBack(strip = true)

    @Test
    fun `a title page with art under the status bar shows the link and it goes back`() = linkGoesBack(strip = false)

    @Test
    fun `a page without the link draws none`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneShell(selected = Route.Home, showTabs = false, onSelect = {}) {} }
        }
        compose.onAllNodesWithText("Back").assertCountEquals(0)
    }
}
