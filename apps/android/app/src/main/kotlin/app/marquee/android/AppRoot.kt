package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import androidx.navigation3.ui.NavDisplay
import app.marquee.core.Format
import app.marquee.core.model.Tile
import app.marquee.core.model.playVideoId
import app.marquee.core.route.Route
import app.marquee.core.route.isTopLevel
import app.marquee.core.route.switchedTo
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.ui.phone.PhoneHome
import app.marquee.ui.phone.PhoneShell
import app.marquee.ui.tv.TvHome
import app.marquee.ui.tv.TvShell
import app.marquee.ui.tv.firstTarget
import app.marquee.ui.tv.focusFirstRow
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** The whole app below the theme: the start decision, the frame, the pages, and the core-stopped cover. */
@Composable
fun MarqueeRoot(
    session: CoreSession,
    form: Form,
    /** A debug launch can start with a deeper stack (a title on Home); null starts on Home. */
    debugStack: List<Route>? = null,
) {
    val caps = (LocalContext.current.applicationContext as MarqueeApp).deviceCaps
    val app = viewModel<AppState>(factory = viewModelFactory { initializer { AppState(session, caps) } })
    Box(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }) {
        when (app.start) {
            Start.Loading -> Unit
            Start.Onboarding -> AppNav(session, form, app, listOf(Route.Onboarding))
            Start.Home -> AppNav(session, form, app, debugStack ?: listOf(Route.Home))
        }
        CoreStoppedOverlay(session)
    }
}

/**
 * TV, on Home: Back moves focus to the first tile of the first lane unless it is there already or
 * there is no tile to move to (Home still loading, empty or failed), when Back leaves as usual.
 * Its own composable, so the focus flag it reads recomposes only this and never the page.
 */
@Composable
private fun HomeBackHandler(
    app: AppState,
    active: Boolean,
) {
    if (!active) return
    val scope = rememberCoroutineScope()
    val running = remember { arrayOfNulls<Job>(1) }
    val reachable = firstTarget(app.home.rows.toList(), app.home.hero, patient = false) != null
    BackHandler(enabled = reachable && !app.tvHome.focus.onFirstRow.value) {
        // A held Back repeats: one move of focus at a time, then the next press leaves.
        if (running[0]?.isActive != true) running[0] = scope.launch { focusFirstRow(app.home, app.tvHome) }
    }
}

@Composable
private fun AppNav(
    session: CoreSession,
    form: Form,
    app: AppState,
    initial: List<Route>,
) {
    val backStack = rememberNavBackStack(*initial.toTypedArray())
    val routes = backStack.map { it as Route }
    val top = routes.last()
    val tv = form == Form.Tv

    fun open(tile: Tile) {
        backStack.add(Route.Title(tile.kind, tile.id))
    }

    // The hero's Play: straight to the player, which picks the recommended version once it settles.
    fun playTile(tile: Tile) {
        backStack.add(Route.Player(tile.kind, tile.id, tile.playVideoId(), tile.name))
    }

    // The player's own ways out. Versions and the next episode replace it (Back should not return to
    // a finished or abandoned player); Episodes goes back to the title page, below or new.
    fun replacePlayer(
        current: Route.Player,
        with: Route,
    ) {
        if (backStack.lastOrNull() != current) return
        backStack.removeAt(backStack.lastIndex)
        backStack.add(with)
    }

    fun toTitle(current: Route.Player) {
        if (backStack.lastOrNull() != current) return
        while (backStack.lastOrNull().let {
                it is Route.Player || it is Route.Versions
            }
        ) {
            backStack.removeAt(backStack.lastIndex)
        }
        // A title page opened from Calendar carries its season: it is the same page.
        val below = backStack.lastOrNull()
        if (below !is Route.Title || below.kind != current.kind || below.id != current.id) {
            backStack.add(Route.Title(current.kind, current.id))
        }
    }

    // The shared Versions views fan out to the addons while open: close them when no screen that
    // needs one is left (the title page, Versions, the player).
    val needsVersions = routes.any { it is Route.Title || it is Route.Versions || it is Route.Player }
    LaunchedEffect(needsVersions) { if (!needsVersions) app.versions.closeAll() }

    fun choose(place: Route) {
        val next = switchedTo(routes, place)
        backStack.clear()
        backStack.addAll(next)
    }

    fun arrive() {
        backStack.clear()
        backStack.add(Route.Home)
    }

    // Back, in one place. Order: overlays and the player (none yet), the stack (NavDisplay's own
    // handler, which calls onBack below), and at the root the system default: the activity
    // finishes and the process stays cached for a quick return. On TV the first Back on Home
    // first moves focus to the first row, so a second Back leaves.
    HomeBackHandler(app, active = tv && top == Route.Home)
    // Home stays composed under a title page while it fades: what focus does there must not change where Back returns to.
    SideEffect { app.tvHome.focus.paused = top != Route.Home }

    val content: @Composable () -> Unit = {
        val fade =
            fadeIn(
                androidx.compose.animation.core
                    .tween(Marquee.panelMs),
            ) togetherWith
                fadeOut(
                    androidx.compose.animation.core
                        .tween(Marquee.panelMs),
                )
        NavDisplay(
            backStack = backStack,
            onBack = { if (backStack.size > 1) backStack.removeAt(backStack.lastIndex) },
            entryDecorators =
                listOf(
                    rememberSaveableStateHolderNavEntryDecorator(),
                    rememberViewModelStoreNavEntryDecorator(),
                ),
            transitionSpec = { fade },
            popTransitionSpec = { fade },
            predictivePopTransitionSpec = { fade },
            entryProvider =
                entryProvider {
                    entry<Route.Onboarding> { OnboardingScreen(session, onDone = ::arrive) }
                    entry<Route.Home> {
                        if (tv) {
                            TvHome(app.home, app.tvHome, onOpen = ::open, onPlay = ::playTile)
                        } else {
                            PhoneHome(app.home, onOpen = ::open, onPlay = ::playTile)
                        }
                    }
                    entry<Route.Search> { SearchScreen(session, ::open) }
                    entry<Route.Discover> { DiscoverScreen(session, ::open) }
                    entry<Route.Library> { LibraryScreen(session, ::open) }
                    entry<Route.Calendar> {
                        CalendarScreen(
                            session,
                            onOpen = { backStack.add(Route.Title("series", it.itemId, it.season)) },
                            onDiscover = { choose(Route.Discover) },
                        )
                    }
                    entry<Route.Settings> { SettingsScreen(session) }
                    entry<Route.Title> { route ->
                        TitleScreen(
                            route,
                            session,
                            app,
                            form,
                            onPlay = { key, videoId, title ->
                                backStack.add(Route.Player(route.kind, route.id, videoId, title, key))
                            },
                            onVersions = { videoId, name ->
                                backStack.add(Route.Versions(route.kind, route.id, videoId, name))
                            },
                            onPlayEpisode = { videoId, name ->
                                backStack.add(Route.Player(route.kind, route.id, videoId, name))
                            },
                            onAddSource = { choose(Route.Settings) },
                        )
                    }
                    entry<Route.Versions> { route ->
                        VersionsScreen(
                            route,
                            app,
                            form,
                            onPlay = { row ->
                                backStack.add(Route.Player(route.kind, route.id, route.videoId, route.title, row.key))
                            },
                            onAddSource = { choose(Route.Settings) },
                        )
                    }
                    entry<Route.Player> { route ->
                        PlayerScreen(
                            route,
                            session,
                            app,
                            form,
                            // Leaving (fading out) it lets Back through to the page that is coming.
                            active = backStack.lastOrNull() == route,
                            // Only this entry pops itself: a held or double Back reaches the leaving entry
                            // again during its fade, and must not pop the screen under it.
                            onBack = { if (backStack.lastOrNull() == route) backStack.removeAt(backStack.lastIndex) },
                            onVersions = {
                                replacePlayer(route, Route.Versions(route.kind, route.id, route.videoId, route.title))
                            },
                            onTitle = if (Format.parseVideo(route.videoId) != null) ({ toTitle(route) }) else null,
                            onNext = { next, binge ->
                                replacePlayer(
                                    route,
                                    Route.Player(
                                        route.kind,
                                        route.id,
                                        next.videoId,
                                        route.title,
                                        key = null,
                                        binge = binge,
                                    ),
                                )
                            },
                        )
                    }
                },
        )
    }

    val selected = routes.lastOrNull { it.isTopLevel } ?: Route.Home
    val framed = top.isTopLevel
    if (tv) {
        TvShell(
            selected = selected,
            showNav = framed,
            onSelect = ::choose,
            contentEntry = { if (top == Route.Home) app.tvHome.focus.lastEntry() else null },
            onNavFocus = { app.tvHome.focus.leftContent() },
            onKey = { app.tvHome.focus.interacted() },
            content = content,
        )
    } else {
        PhoneShell(selected = selected, showTabs = framed, onSelect = ::choose, content = content)
    }
}
