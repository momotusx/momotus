package app.marquee.android

import android.app.Application
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.text.BasicText
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.navigation3.runtime.rememberNavBackStack
import app.marquee.core.route.Route
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * After the process died the picker comes first: a back stack saved under one profile never comes back
 * under the next one picked (journey review P1-1), but does under the same profile.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class ProfileKeyedTest {
    @get:Rule
    val compose = createComposeRule()

    // Plain, not state: the profile changes only for the composition the restore creates.
    private var profile = "dad"

    private fun restoreAs(next: String) {
        val tester = StateRestorationTester(compose)
        tester.setContent {
            ProfileKeyed(profile) {
                val stack = rememberNavBackStack(Route.Home)
                Column {
                    BasicText("top: ${stack.last()}")
                    BasicText("Open", Modifier.clickable { stack.add(Route.Title("movie", "tt1")) })
                }
            }
        }
        compose.onNodeWithText("Open").performClick()
        compose.onNodeWithText("top: ${Route.Title("movie", "tt1")}").assertExists()
        profile = next
        tester.emulateSavedInstanceStateRestore()
    }

    @Test
    fun `another profile picked after the process died starts on its own Home`() {
        restoreAs("kid")
        compose.onNodeWithText("top: ${Route.Home}").assertExists()
    }

    @Test
    fun `the same profile picked again gets its stack back`() {
        restoreAs("dad")
        compose.onNodeWithText("top: ${Route.Title("movie", "tt1")}").assertExists()
    }
}
