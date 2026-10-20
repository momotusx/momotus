package app.marquee.android

import android.app.Application
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
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

    // The profile picked later in the same process (state: a switch recomposes); null until then.
    private var switched by mutableStateOf<String?>(null)

    // What this process's state saw; a new one after the process died.
    private var seen = HashSet<String?>()

    @Test
    fun `a profile switched back to later in the process gets its screens without the player`() {
        val tester = StateRestorationTester(compose)
        tester.setContent {
            val who = switched ?: profile
            ProfileKeyed(who) {
                val stack = rememberProfileStack(who, listOf(Route.Home), seen)
                Column {
                    BasicText("top: ${stack.last()}")
                    BasicText(
                        "Play",
                        Modifier.clickable {
                            stack.add(Route.Title("movie", "tt1"))
                            stack.add(Route.Player("movie", "tt1", "tt1", "Sintel"))
                        },
                    )
                }
            }
        }
        compose.onNodeWithText("Play").performClick()
        // Dad was in the player; the process dies; the kid is picked first.
        profile = "kid"
        seen = HashSet()
        tester.emulateSavedInstanceStateRestore()
        compose.onNodeWithText("top: ${Route.Home}").assertExists()
        // Later in the same process the kid switches to Dad: his screens come back, the player does not.
        compose.runOnIdle { switched = "dad" }
        compose.onNodeWithText("top: ${Route.Title("movie", "tt1")}").assertExists()
    }
}
