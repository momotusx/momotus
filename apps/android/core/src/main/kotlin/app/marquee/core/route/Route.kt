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
    /** First run's first screen (plans/first-screens.md 3.2): over the drifting wall. */
    @Serializable
    data object Welcome : Route

    /** First run's Make your first profile (3.4): names the owner profile every device has, never a new one. */
    @Serializable
    data object FirstProfile : Route

    /** First run's one-page setup (3.5): Stremio or fresh, how to pick what to play, subtitles. */
    @Serializable
    data object Setup : Route

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

    /** One Settings section as its own page, the phone's drill-in (plans/settings-sections.md, "On A1"); [id] is its name. */
    @Serializable
    data class SettingsSection(
        val id: String,
    ) : Route

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

    /**
     * A phone's sign-in (plans/accounts.md 4.1): the providers, Other ways, Not now. [firstRun] from first run; else
     * from Account or a scan asked for before this device had an account. [returning]: from "I already use Momotus",
     * another device's way first (plans/first-screens.md 3.3).
     */
    @Serializable
    data class SignIn(
        val firstRun: Boolean,
        val returning: Boolean = false,
    ) : Route

    /** A TV's first run (4.2): Sign in with your phone, its pairing code and six digits. */
    @Serializable
    data object TvSignIn : Route

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

    /**
     * "Add a source" from a title: the link field first, then the source addons. Pushed over the
     * title, so Back returns to it, and its versions follow the install.
     */
    @Serializable
    data object Sources : Route

    /** Settings > Addons: every installed addon with its switch, order and removal, and the link field. */
    @Serializable
    data object Addons : Route

    /** Find addons: Cinemeta's Official and Community lists, each installed in one press. */
    @Serializable
    data object FindAddons : Route

    /** Settings > Sources > Plugins: Nuvio repositories and their providers, a switch each. */
    @Serializable
    data object Plugins : Route

    /** Find addons > Nuvio plugins: the repositories nuvioplugins.com lists (R8). */
    @Serializable
    data object PluginDirectory : Route

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
        /** Choose the version to download (plans/offline-downloads.md 3.2) instead of the one to play. */
        val download: Boolean = false,
    ) : Route

    /** This device's downloads (plans/offline-downloads.md 3.4): a row at the top of Library on the phone. */
    @Serializable
    data object Downloads : Route

    /** One series' downloaded episodes, by season. */
    @Serializable
    data class DownloadedSeries(
        val itemId: String,
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
        /** Start over on the title page: this play starts at the beginning, not where it was left. */
        val startOver: Boolean = false,
    ) : Route
}

/** The six places the pill or the tab bar leads to. Each shell orders them itself. */
val TOP_LEVEL: List<Route> =
    listOf(Route.Search, Route.Home, Route.Discover, Route.Library, Route.Calendar, Route.Settings)

val Route.isTopLevel: Boolean get() = this in TOP_LEVEL

/**
 * The Settings sections a kids session never shows (plans/m11-kids.md 5): the sources, the account, Home's rows and
 * downloads. The core refuses what they change with `kids`.
 */
val KIDS_HIDDEN_SECTIONS: Set<String> = setOf("Sources", "Account", "Home", "Downloads")

/**
 * Whether a kids session may open this place (plans/m11-kids.md 5, QA round 1 Q7 rows 27 and 32): no Discover or
 * Calendar, no addon, debrid, Trakt, import, account or profile page, and no hidden Settings section. Approving a
 * scan stays: that page says to ask a grown-up.
 */
val Route.openToKids: Boolean
    get() =
        when (this) {
            Route.Discover, Route.Calendar, Route.Import, Route.Pairing, Route.TvSignIn, Route.Sync,
            Route.RecoveryKit, Route.SyncFork, Route.Debrid, Route.Trakt, Route.HomeRows, Route.Sources,
            Route.Addons, Route.FindAddons, Route.Plugins, Route.PluginDirectory, Route.ManageProfiles,
            Route.Welcome, Route.FirstProfile, Route.Setup,
            -> false

            is Route.Defaults, is Route.SignIn, is Route.SyncJoin, is Route.ReportLost, is Route.EditProfile -> false

            is Route.SettingsSection -> id !in KIDS_HIDDEN_SECTIONS

            else -> true
        }

/** The back stack a kids session keeps: the places it may open, Home when none is left. */
fun kidsStack(stack: List<Route>): List<Route> = stack.filter { it.openToKids }.ifEmpty { listOf(Route.Home) }

/**
 * The pushed pages a phone gives the blue back link (design/DECISION.md, Navigation), Calendar among them: the phone
 * pushes it over Library (D6), and only TV has it as a tab, which draws no link. The Downloads pages have it too
 * (plans/offline-downloads.md 3.4). The player, the first run and the Settings sub-pages keep the way out they have.
 */
val Route.hasBackLink: Boolean
    get() =
        this is Route.Title || this is Route.Versions || this is Route.Profiles ||
            this is Route.ManageProfiles || this is Route.EditProfile || this is Route.Calendar ||
            this == Route.Downloads || this is Route.DownloadedSeries || this is Route.SettingsSection

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

/**
 * The back stack after a title page's link to a top-level place (a genre to Discover, a name to Search): the place
 * goes on top, so Back returns to the title. It is taken out of the stack first, since a route appears once.
 */
fun linkedTo(
    stack: List<Route>,
    destination: Route,
): List<Route> = stack.filter { it != destination } + destination

/** Where the recommended addons step was opened from, which decides its rows and where it goes next. */
@Serializable
enum class DefaultsMode {
    /** First run's setup sheet (Start watching, Skip; plans/first-screens.md 3.5): every row as it starts. */
    FirstRun,

    /** After first run's Stremio import: the ones it brought keep how they are, the others go in. */
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
 * On the phone Calendar is a row at the top of Library, pushed over it (D6): a stack stored with Calendar
 * as its place, from when it was a tab, opens Library instead.
 */
fun calendarInLibrary(stack: List<Route>): List<Route> =
    if (stack.getOrNull(1) == Route.Calendar) listOf(stack[0], Route.Library) + stack.drop(2) else stack

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

/**
 * The videos whose Versions page or player is on [stack]. A kind chosen on Versions (plans/plugins-and-marketplace.md
 * 4.2) holds while one of them is: the player plays within it. Once a video leaves this set (back to its title page,
 * a player that ends on the title, any other place), every kind of it is listed again, so the title's Play plays the
 * core's pick.
 */
fun choosingVideos(stack: List<Route>): Set<String> =
    stack.mapNotNullTo(HashSet()) {
        when (it) {
            is Route.Versions -> it.videoId
            is Route.Player -> it.videoId
            else -> null
        }
    }
