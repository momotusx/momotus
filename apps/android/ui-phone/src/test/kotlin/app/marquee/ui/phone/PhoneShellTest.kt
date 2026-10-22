package app.marquee.ui.phone

import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
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
}
