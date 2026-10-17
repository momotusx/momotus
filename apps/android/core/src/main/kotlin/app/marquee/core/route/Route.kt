package app.marquee.core.route

import androidx.navigation3.runtime.NavKey
import kotlinx.serialization.Serializable

/**
 * Every destination of the app; the back stack is a list of these. Same names as the web
 * router. Screens that take a title carry its kind and id, never a view object, so a route can
 * be saved and restored.
 */
@Serializable
sealed interface Route : NavKey {
    @Serializable
    data object Onboarding : Route

    @Serializable
    data object Home : Route

    @Serializable
    data object Search : Route

    @Serializable
    data object Discover : Route

    @Serializable
    data object Library : Route

    @Serializable
    data object Settings : Route

    @Serializable
    data class Title(
        val kind: String,
        val id: String,
    ) : Route
}

/** The five places the rail or the tab bar leads to, in display order. */
val TOP_LEVEL: List<Route> = listOf(Route.Search, Route.Home, Route.Discover, Route.Library, Route.Settings)

val Route.isTopLevel: Boolean get() = this in TOP_LEVEL

/**
 * The back stack after choosing a top-level destination: Home is the root, and the chosen place
 * sits on it, replacing whatever top-level place and titles were above.
 */
fun switchedTo(
    stack: List<Route>,
    destination: Route,
): List<Route> {
    val root = stack.firstOrNull() ?: Route.Home
    return if (destination == root) listOf(root) else listOf(root, destination)
}
