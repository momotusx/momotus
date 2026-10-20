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

    /** The recommended addons step (plans/m13-default-addons.md 6), opened as [mode] says. */
    @Serializable
    data class Defaults(
        val mode: DefaultsMode,
    ) : Route

    /** Join a sync account: this device shows a code another device approves. */
    @Serializable
    data object Pairing : Route

    /** Account (the sync page): this device's account, or the ways to join one. */
    @Serializable
    data object Sync : Route

    /** Start a sync account, or ([kit]) join one with a recovery kit. */
    @Serializable
    data class SyncJoin(
        val kit: Boolean,
    ) : Route

    /** Approve a new device with the code it shows. */
    @Serializable
    data object AddDevice : Route

    /** Make a recovery kit, shown once. */
    @Serializable
    data object RecoveryKit : Route

    /** Compare this device's list with the sync server's, after a fork. */
    @Serializable
    data object SyncFork : Route

    /** Report the account's device [id] lost. */
    @Serializable
    data class ReportLost(
        val id: String,
    ) : Route

    /** Debrid accounts: a key per service, kept on this device. */
    @Serializable
    data object Debrid : Route

    /** The profile's Trakt account: connect, its state, Sync now, Sign out, what did not match. */
    @Serializable
    data object Trakt : Route

    /** Which catalogs Home shows after Continue Watching, and their order. */
    @Serializable
    data object HomeRows : Route

    /** How Marquee picks the version to play (plans/m13-stream-choice.md 4.4). */
    @Serializable
    data object StreamChoice : Route

    /** The one question of a first run, after the recommended addons: start fast or best quality (4.5). */
    @Serializable
    data object StreamQuestion : Route

    /**
     * "Add a source" from a title: the link field first, then the source addons. Pushed over the
     * title, so Back returns to it, and its versions follow the install.
     */
    @Serializable
    data object Sources : Route

    /** Find addons: Cinemeta's Official and Community lists, each installed in one press. */
    @Serializable
    data object FindAddons : Route

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
     * nothing when the title page or Versions already opened it). A stack restored after the process
     * died never brings it back ([restoredStack]).
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

/** Where the recommended addons step was opened from, which decides its rows and where it goes next. */
@Serializable
enum class DefaultsMode {
    /** First run (Start fresh, Browse first) or setup from an empty Home: every row, then Home. */
    FirstRun,

    /** After a Stremio import: the ones it brought say so instead of a switch, then Home. */
    AfterImport,

    /** The Home card's Review: the missing ones only, then back. */
    Review,

    /** Settings: every row with its current switch, then back. */
    Settings,
}

/**
 * A back stack the saved state brought back after the process died: the player, and whatever was over
 * it, do not come back (plans/m6-android.md 7.1), so nothing starts playing by itself and the live
 * views it needs are not assumed. Its title page stands in, with Resume one press away.
 */
fun restoredStack(stack: List<Route>): List<Route> {
    val at = stack.indexOfFirst { it is Route.Player }
    if (at < 0) return stack
    val player = stack[at] as Route.Player
    // A Versions page the version was chosen on leaves with the player: its title page is under it.
    val kept = stack.take(at).dropLastWhile { it is Route.Versions }
    val below = kept.lastOrNull()
    // A title page opened from Calendar carries its season: it is the same page.
    val onTitle = below is Route.Title && below.kind == player.kind && below.id == player.id
    return if (onTitle) kept else kept + Route.Title(player.kind, player.id)
}

/**
 * The stack after a version was chosen on the Versions page at its top. Opened from the player, which
 * waits paused under it, the page gives way with that player to the chosen version; otherwise the
 * chosen version plays over the page.
 */
fun afterChoice(
    stack: List<Route>,
    chosen: Route.Player,
): List<Route> {
    val versions = stack.lastOrNull() as? Route.Versions ?: return stack + chosen
    val below = stack.getOrNull(stack.lastIndex - 1)
    val overPlayer = below is Route.Player && below.id == versions.id && below.videoId == versions.videoId
    return if (overPlayer) stack.dropLast(2) + chosen else stack + chosen
}
