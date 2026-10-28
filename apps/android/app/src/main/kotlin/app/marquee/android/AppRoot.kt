package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.activity.compose.ReportDrawnWhen
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.LocalViewModelStoreOwner
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import androidx.navigation3.ui.NavDisplay
import app.marquee.core.AccountRules
import app.marquee.core.Format
import app.marquee.core.PendingRemoval
import app.marquee.core.TileCopy
import app.marquee.core.model.FirstRunStill
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.Tile
import app.marquee.core.model.playVideoId
import app.marquee.core.route.DefaultsMode
import app.marquee.core.route.Route
import app.marquee.core.route.afterChoice
import app.marquee.core.route.calendarInLibrary
import app.marquee.core.route.hasBackLink
import app.marquee.core.route.isTopLevel
import app.marquee.core.route.linkedTo
import app.marquee.core.route.restoredStack
import app.marquee.core.route.switchedTo
import app.marquee.core.session.CoreSession
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.design.LocalOverPicture
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.actionWindow
import app.marquee.ui.phone.PhoneHome
import app.marquee.ui.phone.PhoneProfileTab
import app.marquee.ui.phone.PhoneShell
import app.marquee.ui.tv.TvHome
import app.marquee.ui.tv.TvNavFocus
import app.marquee.ui.tv.TvShell
import app.marquee.ui.tv.firstTarget
import app.marquee.ui.tv.focusFirstRow
import app.marquee.ui.tv.shownHero
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
    val marquee = LocalContext.current.applicationContext as MarqueeApp
    val gate = marquee.profiles
    val app =
        viewModel<AppState>(
            factory =
                viewModelFactory {
                    initializer {
                        AppState(
                            session,
                            marquee.deviceCaps,
                            gate.shutCount,
                            marquee.lowMemory,
                        )
                    }
                },
        )
    // Each shut of the gate (a switch, a lock, a core that stopped): once the screens have left
    // composition, everything of the old profile goes, and the gate hears so. The count lives in
    // AppState, so a configuration change between the shut and this effect still resets once.
    LaunchedEffect(gate.shutCount) {
        if (gate.shutCount > app.resetAt) app.resetProfile(gate.shutCount, home = !gate.restarted)
        gate.resetDone(app.resetAt)
    }
    // D13: a removal the core holds goes on (once, on the owner of a device with no account), and one that went
    // through starts the app as on a new device.
    val accounts = marquee.accounts
    LaunchedEffect(gate.snapshot?.wipePending, gate.snapshot?.active, marquee.sync.joined) { accounts.resumeRemoval() }
    LaunchedEffect(accounts.removal.wiped) { if (accounts.removal.wiped) app.wiped() }
    Box(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }) {
        val locked = gate.snapshot?.locked == true
        when {
            gate.snapshot == null && gate.problem != null -> {
                // The profiles view failed to come back; the gate tries again meanwhile.
                StateMessage(title = "Profiles did not load", body = "${gate.problem}. Trying again.")
            }

            app.start == Start.Loading || gate.snapshot == null -> {
                StartingScreen()
            }

            !gate.open -> {
                PickerScreen(gate, session, if (locked) PickerMode.Locked else PickerMode.Start)
            }

            else -> {
                CompositionLocalProvider(LocalViewModelStoreOwner provides app.screens) {
                    ProfileKeyed(gate.snapshot?.active) {
                        when (app.start) {
                            Start.Loading -> Unit
                            Start.Onboarding -> AppNav(session, form, app, firstRun(accounts.server(marquee), form))
                            Start.Home -> AppNav(session, form, app, debugStack ?: listOf(Route.Home))
                        }
                    }
                }
            }
        }
        CoreStoppedOverlay(session)
    }
}

/**
 * A first run's stack (plans/accounts.md 4.1, 4.2): the doors, with the sign-in over them where a sync service is
 * known (Not now and Back lead to the doors).
 */
internal fun firstRun(
    server: String,
    form: Form,
): List<Route> =
    when {
        server.isEmpty() -> listOf(Route.Onboarding)
        form == Form.Tv -> listOf(Route.Onboarding, Route.TvSignIn)
        else -> listOf(Route.Onboarding, Route.SignIn(firstRun = true))
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
    val reachable = firstTarget(app.home.rows.toList(), app.tvHome.shownHero(app.home), patient = false) != null
    BackHandler(enabled = reachable && !app.tvHome.focus.onFirstRow.value) {
        // A held Back repeats: one move of focus at a time, then the next press leaves.
        if (running[0]?.isActive != true) running[0] = scope.launch { focusFirstRow(app.home, app.tvHome) }
    }
}

/**
 * One profile's navigation. Keyed by the profile, so what it saved (the back stack, each screen's
 * state) is never restored under another: after the process died the picker comes first, and whoever
 * picks next may be someone else (journey review P1-1).
 */
@Composable
internal fun ProfileKeyed(
    profile: String?,
    content: @Composable () -> Unit,
) = key(profile) { content() }

/**
 * The back stack of [profile]. The first one each profile composes in this state may be what the saved state
 * brought back after the process died (picked first, or later in the process): it does not reopen the player
 * (plans/m6-android.md 7.1, journey review r2 P2-1). [seen] holds the profiles already past that.
 */
@Composable
internal fun rememberProfileStack(
    profile: String?,
    initial: List<Route>,
    seen: MutableSet<String?>,
    /** The phone has no Calendar tab: a stored one opens Library. */
    phone: Boolean = false,
) = rememberNavBackStack(*initial.toTypedArray()).let { saved ->
    remember(saved) {
        if (seen.add(profile)) {
            val restored = saved.map { it as Route }
            val kept = restoredStack(restored).let { if (phone) calendarInLibrary(it) else it }
            if (kept != restored) {
                saved.clear()
                saved.addAll(kept)
            }
        }
        saved
    }
}

@Composable
private fun AppNav(
    session: CoreSession,
    form: Form,
    app: AppState,
    initial: List<Route>,
) {
    val marquee = LocalContext.current.applicationContext as MarqueeApp
    val sync = (LocalContext.current.applicationContext as MarqueeApp).sync
    val gate = (LocalContext.current.applicationContext as MarqueeApp).profiles
    val accessibility = LocalAccessibilityManager.current
    val backStack = rememberProfileStack(gate.snapshot?.active, initial, app.stacksSeen, phone = form == Form.Phone)
    val routes = backStack.map { it as Route }
    val top = routes.last()
    val tv = form == Form.Tv
    // The phone's back link is the system Back: a page that guards leaving (an edited profile) is asked first.
    val systemBack = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher

    fun open(tile: Tile) {
        backStack.add(Route.Title(tile.kind, tile.id))
    }

    // The hero's Play: straight to the player, which picks the recommended version once it settles.
    fun playTile(tile: Tile) {
        backStack.add(Route.Player(tile.kind, tile.id, tile.playVideoId(), tile.name))
    }

    // The player's own ways out. The next episode and an offered version replace it (Back should not
    // return to a finished or abandoned player); Versions opens over it; Episodes goes back to the title
    // page, below or new.
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

    // A title page's link (a genre, a name) keeps the title under the place it opens: Back returns to it.
    fun link(place: Route) {
        val next = linkedTo(routes, place)
        backStack.clear()
        backStack.addAll(next)
    }

    fun arrive() {
        backStack.clear()
        backStack.add(Route.Home)
        app.onboarded()
    }

    fun pop() {
        if (backStack.size > 1) backStack.removeAt(backStack.lastIndex)
    }

    // An addon's Install button (stremio://): the page with the link field, filled in, waits for Add.
    LaunchedEffect(marquee.addonLink) {
        val link = marquee.addonLink ?: return@LaunchedEffect
        marquee.addonLink = null
        app.linkInstall.offered = link
        if (backStack.lastOrNull() != Route.Sources) backStack.add(Route.Sources)
    }

    // A join from the first screen settles how to begin: back on it, the app goes Home. An account created there
    // goes on through the doors (I use Stremio or Start fresh, 4.1.5).
    var created by rememberSaveable { mutableStateOf(false) }
    val onFirstScreen = backStack.singleOrNull() == Route.Onboarding
    LaunchedEffect(sync.joined, onFirstScreen) {
        if (sync.joined && onFirstScreen && !created) arrive()
    }

    // Sign in another device (the app shortcut, or a phone camera on a TV's QR): the scanner, after the sign-in
    // when this device has no account yet. A TV approves nothing (D6), and a kids profile reaches no account page.
    val signingIn = routes.any { it is Route.SignIn }
    LaunchedEffect(marquee.pendingScan, sync.joined, signingIn) {
        when {
            !marquee.pendingScan || signingIn -> {
                Unit
            }

            tv || AccountRules.activeProfile(gate.snapshot)?.kids == true -> {
                marquee.pendingScan = false
            }

            sync.joined -> {
                marquee.pendingScan = false
                if (marquee.accounts.rows(tv).approve && backStack.lastOrNull() != Route.AddDevice) {
                    backStack.add(Route.AddDevice)
                }
            }

            else -> {
                backStack.add(Route.SignIn(firstRun = Route.Onboarding in routes))
            }
        }
    }

    // Back, in one place. Order: overlays and the player (none yet), the stack (NavDisplay's own
    // handler, which calls onBack below), and at the root the system default: the activity
    // finishes and the process stays cached for a quick return. On TV the first Back on Home
    // first moves focus to the first row, so a second Back leaves.
    HomeBackHandler(app, active = tv && top == Route.Home)
    val tvNav = remember { TvNavFocus() }
    // Home stays composed under a title page while it fades: what focus does there must not change where Back returns to.
    SideEffect { app.tvHome.focus.paused = top != Route.Home }
    // Discover too: a title page's own first focus must not overwrite the tile Back returns to.
    SideEffect { app.discoverFocus.paused = top != Route.Discover }

    val content: @Composable () -> Unit = {
        val fade = screenFade(Marquee.panelMs)
        NavDisplay(
            backStack = backStack,
            onBack = {
                // TV: Back in a place's content first puts focus on its tab, as Menu does on Apple TV; Back on the bar
                // then goes Home. Home keeps its own chain (HomeBackHandler).
                val toBar = tv && top.isTopLevel && tvNav.backToBar()
                if (!toBar && backStack.size > 1) backStack.removeAt(backStack.lastIndex)
            },
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
                    entry<Route.Onboarding> {
                        OnboardingScreen(
                            onDefaults = { backStack.add(Route.Defaults(DefaultsMode.FirstRun)) },
                            onImport = { backStack.add(Route.Import) },
                            onPair = if (created) null else ({ backStack.add(Route.Pairing) }),
                            still = app.firstRunStill(),
                        )
                    }
                    entry<Route.SignIn> { route ->
                        SignInScreen(
                            marquee.accounts,
                            session,
                            sync,
                            route.firstRun,
                            onNotNow = {
                                marquee.pendingScan = false
                                pop()
                            },
                            onScan = { backStack.add(Route.Pairing) },
                            onKit = { backStack.add(Route.SyncJoin(kit = true)) },
                            onNoProviders = {
                                pop()
                                backStack.add(Route.Sync)
                            },
                            onCreated = { kit ->
                                if (route.firstRun) created = true
                                pop()
                                if (kit) backStack.add(Route.RecoveryKit)
                            },
                            onIn = { if (route.firstRun) arrive() else pop() },
                        )
                    }
                    entry<Route.TvSignIn> {
                        TvSignInScreen(
                            session,
                            sync,
                            marquee.accounts.server(marquee),
                            onDoors = ::pop,
                            onKit = { backStack.add(Route.SyncJoin(kit = true)) },
                            onIn = ::arrive,
                        )
                    }
                    entry<Route.Import> {
                        // From the first screen a finished import goes on to the recommended addons.
                        val firstRun = Route.Onboarding in routes
                        ImportScreen(
                            session,
                            onDone = {
                                if (firstRun) {
                                    backStack.add(
                                        Route.Defaults(DefaultsMode.AfterImport),
                                    )
                                } else {
                                    arrive()
                                }
                            },
                            toDefaults = firstRun,
                        )
                    }
                    entry<Route.Defaults> { route ->
                        DefaultsScreen(session, route.mode, onBack = ::pop, onDone = {
                            when (route.mode) {
                                // A first run asks the one question (plans/m13-stream-choice.md 4.5).
                                DefaultsMode.FirstRun, DefaultsMode.AfterImport -> backStack.add(Route.StreamQuestion)

                                DefaultsMode.Review, DefaultsMode.Settings -> pop()
                            }
                        })
                    }
                    // The subtitles step takes the question's place: Back from it goes to the addons.
                    entry<Route.StreamQuestion> {
                        StreamQuestionScreen(
                            session,
                            still = app.firstRunStill(),
                            onBack = ::pop,
                            onDone = {
                                // A Back during the save took the question away: nothing to replace.
                                if (backStack.lastOrNull() == Route.StreamQuestion) {
                                    backStack[backStack.lastIndex] = Route.SubtitleLanguage
                                }
                            },
                        )
                    }
                    entry<Route.SubtitleLanguage> {
                        SubtitleStepScreen(session, still = app.firstRunStill(), onBack = ::pop, onDone = ::arrive)
                    }
                    entry<Route.Home> {
                        // Fully drawn once the first rows show: the start's end for the system and the benchmarks.
                        ReportDrawnWhen { app.home.rows.isNotEmpty() }
                        // An empty Home asks once whether catalogs exist and are all hidden.
                        val empty = app.home.status == ViewStatus.Open && app.home.rows.isEmpty()
                        val rowsHidden by produceState(false, empty) {
                            value = empty &&
                                runCatching { session.ops.homeCatalogs() }
                                    .getOrNull()
                                    ?.let { it.isNotEmpty() && it.all(HomeCatalog::hidden) } == true
                        }
                        val onHomeRows: () -> Unit = { backStack.add(Route.HomeRows) }
                        // New recommended addons, once per list version (plans/m13-default-addons.md 7).
                        // Not beside an empty Home's setup, which offers the same addons.
                        val profile = gate.snapshot
                        LaunchedEffect(profile?.active) {
                            app.defaultsCard.check(kids = profile?.kids != false) { defaultsSeen(session) }
                        }
                        // Home's one account card slot comes first (plans/accounts.md 4.3): a scheduled deletion
                        // is the one thing that must not wait behind the addons card.
                        val accountCard: (@Composable () -> Unit)? =
                            marquee.accounts.homeCard()?.let { shown ->
                                {
                                    AccountCard(
                                        marquee.accounts,
                                        sync,
                                        shown,
                                        onSignIn = {
                                            backStack.add(if (tv) Route.TvSignIn else Route.SignIn(firstRun = false))
                                        },
                                        onMakeKit = { backStack.add(Route.RecoveryKit) },
                                    )
                                }
                            }
                        val addonsCard: (@Composable () -> Unit)? =
                            app.defaultsCard.offer?.takeIf { app.home.rows.isNotEmpty() }?.let { answer ->
                                {
                                    RecommendedCard(
                                        answer,
                                        onReview = {
                                            app.defaultsCard.answer()
                                            backStack.add(Route.Defaults(DefaultsMode.Review))
                                        },
                                        onNotNow = { app.defaultsCard.answer() },
                                    )
                                }
                            }
                        // Both show, the account card first, as on the web.
                        val cards = listOfNotNull(accountCard, addonsCard)
                        val card: (@Composable () -> Unit)? =
                            cards.takeIf { it.isNotEmpty() }?.let {
                                {
                                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
                                        cards.forEach { it() }
                                    }
                                }
                            }
                        val removal = app.removal
                        val notice: (@Composable () -> Unit)? =
                            if (removal.removing != null || removal.failed != null) ({ RemovalNote(removal) }) else null
                        if (tv) {
                            TvHome(
                                app.home,
                                app.tvHome,
                                onOpen = ::open,
                                onPlay = ::playTile,
                                onSetup = { backStack.add(Route.Onboarding) },
                                onAddons = { choose(Route.Settings) },
                                rowsHidden = rowsHidden,
                                onHomeRows = onHomeRows,
                                card = card,
                                removal = app.removal,
                            )
                        } else {
                            PhoneHome(
                                app.home,
                                onOpen = ::open,
                                onPlay = ::playTile,
                                onSetup = { backStack.add(Route.Onboarding) },
                                onAddons = { choose(Route.Settings) },
                                rowsHidden = rowsHidden,
                                onHomeRows = onHomeRows,
                                card = card,
                                onRemove = { app.removal.remove(it, accessibility::actionWindow) },
                                removing = app.removal.removing?.id,
                                notice = notice,
                            )
                        }
                    }
                    entry<Route.Search> {
                        SearchScreen(
                            session,
                            ::open,
                            request = app.searchRequest,
                            onRequestSent = { app.searchRequest = null },
                            onAddons = { backStack.add(Route.Defaults(DefaultsMode.Settings)) },
                        )
                    }
                    entry<Route.Discover> {
                        DiscoverScreen(
                            session,
                            ::open,
                            request = app.discoverRequest,
                            onRequestSent = { app.discoverRequest = null },
                            onSetup = { backStack.add(Route.Onboarding) },
                            onAddons = { choose(Route.Settings) },
                            focus = app.discoverFocus,
                        )
                    }
                    entry<Route.Library> {
                        // On the phone Calendar is a row at the top of Library, pushed over it (D6).
                        LibraryScreen(
                            session,
                            ::open,
                            onCalendar = if (tv) null else ({ backStack.add(Route.Calendar) }),
                            onDiscover = { choose(Route.Discover) },
                        )
                    }
                    entry<Route.Calendar> {
                        CalendarScreen(
                            session,
                            onOpen = { backStack.add(Route.Title("series", it.itemId, it.season)) },
                            onDiscover = { choose(Route.Discover) },
                        )
                    }
                    entry<Route.Profiles> {
                        PickerScreen(
                            gate,
                            session,
                            PickerMode.Switch,
                            onBack = ::pop,
                            onAdd = { backStack.add(Route.EditProfile(null)) },
                            onManage = { backStack.add(Route.ManageProfiles) },
                        )
                    }
                    entry<Route.ManageProfiles> {
                        ManageProfilesScreen(
                            gate,
                            onEdit = { backStack.add(Route.EditProfile(it)) },
                            onAdd = { backStack.add(Route.EditProfile(null)) },
                        )
                    }
                    entry<Route.EditProfile> { route -> EditProfileScreen(gate, session, route.id, onDone = ::pop) }
                    entry<Route.Settings> {
                        SettingsScreen(
                            session,
                            gate,
                            art = settingsArt(app.home.rows, app.home.heroes, kids = gate.snapshot?.kids == true),
                            onProfiles = { backStack.add(Route.Profiles) },
                            onEditProfile = { backStack.add(Route.EditProfile(it)) },
                            onManageProfiles = { backStack.add(Route.ManageProfiles) },
                            sync,
                            onAddons = { backStack.add(Route.Addons) },
                            onImport = { backStack.add(Route.Import) },
                            onSync = { backStack.add(Route.Sync) },
                            onDebrid = { backStack.add(Route.Debrid) },
                            onTrakt = { backStack.add(Route.Trakt) },
                            onHomeRows = { backStack.add(Route.HomeRows) },
                            onFindAddons = { backStack.add(Route.FindAddons) },
                            onStreamChoice = { backStack.add(Route.StreamChoice) },
                            onDefaults = { backStack.add(Route.Defaults(DefaultsMode.Settings)) },
                        )
                    }
                    entry<Route.Addons> {
                        AddonsScreen(
                            session,
                            app.linkInstall,
                            onFindAddons = { backStack.add(Route.FindAddons) },
                            sourcesOnly = false,
                        )
                    }
                    entry<Route.Sources> {
                        AddonsScreen(
                            session,
                            app.linkInstall,
                            onFindAddons = { backStack.add(Route.FindAddons) },
                            sourcesOnly = true,
                        )
                    }
                    entry<Route.FindAddons> {
                        // TV's Paste a link: the page with the link field, back to it when Find addons came from there.
                        val fromSources =
                            routes.getOrNull(routes.lastIndex - 1).let {
                                it == Route.Sources ||
                                    it == Route.Addons
                            }
                        FindAddonsScreen(session, app.installCards, app.linkInstall, onPasteLink = {
                            if (fromSources) pop() else backStack.add(Route.Sources)
                        })
                    }
                    entry<Route.Debrid> { DebridScreen(session) }
                    entry<Route.Trakt> {
                        TraktScreen(
                            session,
                            (LocalContext.current.applicationContext as MarqueeApp).trakt,
                        )
                    }
                    entry<Route.HomeRows> { HomeRowsScreen(session) }
                    entry<Route.StreamChoice> { StreamChoiceScreen(session) }
                    entry<Route.Pairing> {
                        // From onboarding, a join is the way in: Done goes Home. From Settings it goes back.
                        PairingScreen(session, sync, onDone = {
                            when {
                                // Also when setup was opened again from an empty Home or Discover.
                                Route.Onboarding in backStack -> {
                                    arrive()
                                }

                                else -> {
                                    pop()
                                    // Scanned in from a sign-in's Other ways: that sign-in is done with.
                                    if (sync.joined && backStack.lastOrNull() is Route.SignIn) {
                                        marquee.accounts.endSignIn()
                                        pop()
                                    }
                                }
                            }
                        })
                    }
                    entry<Route.Sync> {
                        SyncScreen(
                            sync,
                            marquee.accounts,
                            onPair = { backStack.add(Route.Pairing) },
                            onJoin = { backStack.add(Route.SyncJoin(it)) },
                            onAdd = { backStack.add(Route.AddDevice) },
                            onKit = { backStack.add(Route.RecoveryKit) },
                            onLost = { backStack.add(Route.ReportLost(it)) },
                            onCompare = { backStack.add(Route.SyncFork) },
                            onSignIn = { backStack.add(Route.SignIn(firstRun = false)) },
                            rows = marquee.accounts.rows(tv),
                        )
                    }
                    entry<Route.SyncFork> { ForkScreen(sync, onDone = ::pop) }
                    entry<Route.SyncJoin> { route -> SyncJoinScreen(sync, route.kit, onDone = ::pop) }
                    entry<Route.AddDevice> {
                        AddDeviceScreen(sync, onDone = {
                            marquee.scanAgain = false
                            pop()
                        }, scanAgain = marquee.scanAgain)
                    }
                    entry<Route.RecoveryKit> { RecoveryKitScreen(sync, onDone = ::pop) }
                    entry<Route.ReportLost> { route -> ReportLostScreen(sync, route.id, onDone = ::pop) }
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
                            onGenre = { kind, genre ->
                                app.discoverRequest = DiscoverRequest(kind, genre)
                                link(Route.Discover)
                            },
                            onOpen = ::open,
                            onPerson = { name ->
                                app.searchRequest = name
                                link(Route.Search)
                            },
                        )
                    }
                    entry<Route.Versions> { route ->
                        VersionsScreen(
                            route,
                            app,
                            form,
                            onPlay = { key ->
                                val next =
                                    afterChoice(
                                        routes,
                                        Route.Player(route.kind, route.id, route.videoId, route.title, key),
                                    )
                                backStack.clear()
                                backStack.addAll(next)
                            },
                            onAddSource = { backStack.add(Route.Sources) },
                            onBack = ::pop,
                            onUndo = { session.ops.undoLearned(it.screenClass, it.choice) },
                            // Over the paused player its attempt keeps resolving: the rows hold their order.
                            held = backStack.getOrNull(backStack.indexOf(route) - 1) is Route.Player,
                        )
                    }
                    entry<Route.Player> { route ->
                        // The player's controls sit over the picture with no scrim: they keep an opaque fill. Their TV
                        // focus is the white invert, as everywhere.
                        CompositionLocalProvider(LocalOverPicture provides true) {
                            PlayerScreen(
                                route,
                                session,
                                app,
                                form,
                                // Leaving (fading out) it lets Back through to the page that is coming.
                                active = backStack.lastOrNull() == route,
                                // Only this entry pops itself: a held or double Back reaches the leaving entry
                                // again during its fade, and must not pop the screen under it.
                                onBack = {
                                    val top = backStack.lastOrNull() == route
                                    if (top) backStack.removeAt(backStack.lastIndex)
                                },
                                // Over the player, which waits paused: Back returns to it, a choice takes its place.
                                onVersions = {
                                    if (backStack.lastOrNull() == route) {
                                        backStack.add(Route.Versions(route.kind, route.id, route.videoId, route.title))
                                    }
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
                                // The same video and Versions view, the offered version's key.
                                onOffer = { key -> replacePlayer(route, route.copy(key = key)) },
                                // Where an addon is added (noSource). Accounts are set up in Settings only.
                                onSettings = { backStack.add(Route.Sources) },
                            )
                        }
                    }
                },
        )
    }

    val selected = routes.lastOrNull { it.isTopLevel } ?: Route.Home
    val framed = top.isTopLevel

    // The profile's menu (plans/nav-profile.md): a profile with a PIN opens the picker on its keypad.
    fun pinFor(id: String) {
        gate.asked = id
        backStack.add(Route.Profiles)
    }
    var profileMenu by remember { mutableStateOf(false) }
    val me = gate.snapshot?.let { s -> s.profiles.firstOrNull { it.id == s.active } }
    val phoneProfile =
        me?.let {
            PhoneProfileTab("Profile: ${it.name}", { profileMenu = true }) { ProfileAvatar(it, 26.dp) }
        }
    if (profileMenu && !tv) ProfileMenu(gate, { profileMenu = false }, { choose(Route.Settings) }, ::pinFor)
    if (tv) {
        TvShell(
            selected = selected,
            showNav = framed,
            onSelect = ::choose,
            contentEntry = {
                when (top) {
                    Route.Home -> {
                        app.tvHome.focus.lastEntry()
                    }

                    // Discover: where focus last was, else the selected kind (6.5), never the grid by geometry.
                    Route.Discover -> {
                        app.discoverFocus.lastEntry()
                            ?: app.discoverFocus.rowEntry("kinds")
                            ?: app.discoverFocus.requester(DISCOVER_KIND_ENTRY)
                    }

                    else -> {
                        null
                    }
                }
            },
            onNavFocus = { app.tvHome.focus.leftContent() },
            onKey = { app.tvHome.focus.interacted() },
            nav = tvNav,
            profileButton = { lit, bar -> PillProfile(gate, lit, bar, { choose(Route.Settings) }, ::pinFor) },
            content = content,
        )
    } else {
        PhoneShell(
            selected = selected,
            showTabs = framed,
            onSelect = ::choose,
            statusStrip = top !is Route.Title && top !is Route.Player,
            insetSides = top !is Route.Player,
            onBack = if (top.hasBackLink) ({ systemBack?.onBackPressed() ?: pop() }) else null,
            profile = phoneProfile,
            content = content,
        )
    }
}

/**
 * The cross-fade between screens. Unclipped: by default the fade clips the screens to their box, which on
 * TV cut Home's top shelf art off at the tab bar for the whole fade, then let it jump up behind the bar.
 */
internal fun screenFade(ms: Int): ContentTransform =
    ContentTransform(fadeIn(tween(ms)), fadeOut(tween(ms)), sizeTransform = SizeTransform(clip = false))

/** Under Continue Watching after Remove: the words with Undo while it can be taken back, or why it failed. */
@Composable
private fun RemovalNote(removal: PendingRemoval) {
    val tile = removal.removing
    val words = tile?.let { TileCopy.removed(it.name) } ?: removal.failed ?: return
    Row(
        Modifier.padding(horizontal = Marquee.safe.side),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(
            words,
            role = TextRole.Callout,
            color = Marquee.colors.muted,
            modifier = Modifier.weight(1f, fill = false).semantics { liveRegion = LiveRegionMode.Polite },
        )
        if (tile != null) SecondaryButton("Undo", removal::undo)
    }
}

/** The first run's still (B4): asked at the first framed step that shows, then kept for the others. */
@Composable
private fun AppState.firstRunStill(): FirstRunStill? {
    LaunchedEffect(this) { askStill() }
    return still
}
