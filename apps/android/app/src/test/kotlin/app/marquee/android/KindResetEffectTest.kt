package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createComposeRule
import app.marquee.core.route.Route
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/**
 * AppRoot's side of a chosen kind (plans/plugins-and-marketplace.md 4.2): nothing while the video's Versions page or
 * player is on the stack, one "every kind" for the video once the last of them leaves, whatever the path.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class KindResetEffectTest {
    @get:Rule
    val compose = createComposeRule()

    private val title = Route.Title("movie", "tt1")
    private val versions = Route.Versions("movie", "tt1", "tt1", "Sintel")
    private val player = Route.Player("movie", "tt1", "tt1", "Sintel", key = "a")

    @Test
    fun `the kind holds from Versions to the player and back, and every kind is listed once the video leaves`() {
        var routes by mutableStateOf(listOf<Route>(title, versions))
        val listed = mutableListOf<String>()
        compose.setContent { KindResetEffect(routes) { listed += it } }
        compose.waitForIdle()
        // A choice replaces Versions with the player; Versions comes back over the paused player.
        routes = listOf(title, player)
        compose.waitForIdle()
        routes = listOf(title, player, versions)
        compose.waitForIdle()
        routes = listOf(title, player)
        compose.waitForIdle()
        assertEquals(emptyList(), listed, "the player plays within the kind")
        // The player ends on the title.
        routes = listOf(title)
        compose.waitForIdle()
        assertEquals(listOf("tt1"), listed)
    }

    @Test
    fun `leaving for another place lists every kind of the video too`() {
        var routes by mutableStateOf(listOf<Route>(title, versions))
        val listed = mutableListOf<String>()
        compose.setContent { KindResetEffect(routes) { listed += it } }
        compose.waitForIdle()
        routes = listOf(Route.Settings)
        compose.waitForIdle()
        assertEquals(listOf("tt1"), listed)
    }
}
