package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
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
import app.marquee.core.model.Tile
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
import app.marquee.ui.tv.focusFirstRow
import kotlinx.coroutines.launch

/** The whole app below the theme: the start decision, the frame, the pages, and the core-stopped cover. */
@Composable
fun MarqueeRoot(
    session: CoreSession,
    form: Form,
) {
    val app = viewModel<AppState>(factory = viewModelFactory { initializer { AppState(session) } })
    Box(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }) {
        when (app.start) {
            Start.Loading -> Unit
            Start.Onboarding -> AppNav(session, form, app, Route.Onboarding)
            Start.Home -> AppNav(session, form, app, Route.Home)
        }
        CoreStoppedOverlay(session)
    }
}

@Composable
private fun AppNav(
    session: CoreSession,
    form: Form,
    app: AppState,
    initial: Route,
) {
    val backStack = rememberNavBackStack(initial)
    val scope = rememberCoroutineScope()
    val routes = backStack.map { it as Route }
    val top = routes.last()
    val tv = form == Form.Tv

    fun open(tile: Tile) {
        backStack.add(Route.Title(tile.kind, tile.id))
    }

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
    BackHandler(enabled = tv && top == Route.Home && !app.tvHome.focus.onFirstRow.value) {
        scope.launch { focusFirstRow(app.home, app.tvHome) }
    }

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
                        if (tv) TvHome(app.home, app.tvHome, onOpen = ::open) else PhoneHome(app.home, onOpen = ::open)
                    }
                    entry<Route.Search> { ComingSoonScreen("Search") }
                    entry<Route.Discover> { ComingSoonScreen("Discover") }
                    entry<Route.Library> { ComingSoonScreen("Library") }
                    entry<Route.Settings> { ComingSoonScreen("Settings") }
                    entry<Route.Title> { TitlePlaceholder(it) }
                },
        )
    }

    val selected = routes.lastOrNull { it.isTopLevel } ?: Route.Home
    val framed = top.isTopLevel
    if (tv) {
        TvShell(
            selected = selected,
            showRail = framed,
            onSelect = ::choose,
            contentEntry = { if (top == Route.Home) app.tvHome.focus.lastEntry() else null },
            onRailFocus = { app.tvHome.focus.leftContent() },
            content = content,
        )
    } else {
        PhoneShell(selected = selected, showTabs = framed, onSelect = ::choose, content = content)
    }
}
