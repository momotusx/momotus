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
    data object Calendar : Route

    @Serializable
    data object Settings : Route

    /** Import from Stremio: a code to enter at link.stremio.com. */
    @Serializable
    data object Import : Route

    /** Join a sync account: this device shows a code another device approves. */
    @Serializable
    data object Pairing : Route

    /** The sync account this device is in. */
    @Serializable
    data object Sync : Route

    /** Debrid accounts: a key per service, kept on this device. */
    @Serializable
    data object Debrid : Route

    /** Which catalogs Home shows after Continue Watching, and their order. */
    @Serializable
    data object HomeRows : Route

    /** The picker opened over the app (the pill or Settings): Back returns where it was. */
    @Serializable
    data object Profiles : Route

    /** Manage profiles (the owner). */
    @Serializable
    data object ManageProfiles : Route

    /** A profile's edit page; [id] null adds one. */
    @Serializable
    data class EditProfile(
        val id: String? = null,
    ) : Route

    @Serializable
    data class Title(
        val kind: String,
        val id: String,
        /** The season to open on (a Calendar episode); applied once, when the seasons arrive. */
        val season: Int? = null,
    ) : Route

    /** The ranked sources of one video (a movie, or one episode) of a title. */
    @Serializable
    data class Versions(
        val kind: String,
        val id: String,
        val videoId: String,
        /** The movie or series name, which the ranking needs (never an episode title). */
        val title: String,
    ) : Route

    /**
     * Playing a video. It asks for the ranked versions itself (the view is shared, so this costs
     * nothing when the title page or Versions already opened it), which also lets a restored stack
     * start playing again after the process died.
     */
    @Serializable
    data class Player(
        val kind: String,
        val id: String,
        val videoId: String,
        /** The movie or series name: Versions needs it, and the OSD shows it. */
        val title: String,
        /** The version chosen on Versions or the title page; null plays the recommended one once it settles. */
        val key: String? = null,
        /** The release line of the episode that just played, so the next one comes from the same line. */
        val binge: String? = null,
    ) : Route
}

/** The six places the pill or the tab bar leads to. Each shell orders them itself. */
val TOP_LEVEL: List<Route> =
    listOf(Route.Search, Route.Home, Route.Discover, Route.Library, Route.Calendar, Route.Settings)

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
