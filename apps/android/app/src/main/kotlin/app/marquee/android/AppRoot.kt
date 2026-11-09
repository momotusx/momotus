package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.activity.compose.ReportDrawnWhen
import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
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
import app.marquee.core.AccountCopy
import app.marquee.core.AccountRules
import app.marquee.core.FirstRun
import app.marquee.core.FirstRunCopy
import app.marquee.core.Format
import app.marquee.core.PendingRemoval
import app.marquee.core.ProfileCopy
import app.marquee.core.TileCopy
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.Tile
import app.marquee.core.model.playVideoId
import app.marquee.core.route.DefaultsMode
import app.marquee.core.route.Route
import app.marquee.core.route.afterChoice
import app.marquee.core.route.calendarInLibrary
import app.marquee.core.route.choosingVideos
import app.marquee.core.route.hasBackLink
import app.marquee.core.route.isTopLevel
import app.marquee.core.route.kidsStack
import app.marquee.core.route.linkedTo
import app.marquee.core.route.restoredStack
import app.marquee.core.route.switchedTo
import app.marquee.core.session.CoreSession
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.design.LocalIncognito
import app.marquee.design.LocalOverPicture
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.Status
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
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

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
    val incognito = ProfileCopy.inIncognito(gate.snapshot)
    // Who's watching? over the wall ends with its answer.
    LaunchedEffect(gate.open) { if (gate.open) app.who = false }
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
                val mode = if (locked) PickerMode.Locked else PickerMode.Start
                // A first-run join (D2), or a locked first start: the picker is a sheet over the still wall.
                if (app.who || app.start == Start.Onboarding) {
                    LaunchedEffect(Unit) { app.askWall() }
                    Box(Modifier.fillMaxSize()) {
                        FirstWall(app.wall, app.drift, sheet = true)
                        PickerScreen(
                            gate,
                            session,
                            mode,
                            // A profile added in the sheet: Home, on the profile this session runs.
                            onAdd = gate::markChosen,
                            sheet = true,
                            line = whoLine(app, marquee.sync),
                        )
                    }
                } else {
                    PickerScreen(gate, session, mode)
                }
            }

            // Every Incognito entry opens on its intro (plans/batch-10-incognito.md 5.1).
            incognito && gate.intro -> {
                // The debrid line never depends on timing: the intro waits for the debrid view (it opens at start).
                val debrid =
                    introDebrid(
                        session.debrid
                            .current()
                            ?.accounts
                            ?.size,
                        session.debrid.status is ViewStatus.Failed,
                    )
                if (debrid == null) {
                    StartingScreen()
                } else {
                    IncognitoIntro(
                        debrid = debrid,
                        onContinue = { gate.intro = false },
                        onLeave = {
                            gate.intro = false
                            gate.startLeave()
                        },
                    )
                }
            }

            else -> {
                CompositionLocalProvider(
                    LocalViewModelStoreOwner provides app.screens,
                    LocalIncognito provides incognito,
                ) {
                    ProfileKeyed(gate.snapshot?.active) {
                        when (app.start) {
                            Start.Loading -> {
                                Unit
                            }

                            Start.Onboarding -> {
                                AppNav(
                                    session,
                                    form,
                                    app,
                                    FirstRunNav.initial(
                                        firstState(gate.snapshot, accounts.server(marquee), marquee.sync.joined),
                                    ),
                                )
                            }

                            Start.Home -> {
                                AppNav(session, form, app, debugStack ?: listOf(Route.Home))
                            }
                        }
                    }
                }
            }
        }
        CoreStoppedOverlay(session)
    }
}

/** First run's inputs while the profile is fresh (plans/first-screens.md 4.3): the core's and this device's state. */
internal fun firstState(
    s: ProfilesSnapshot?,
    server: String,
    joined: Boolean,
) = FirstRun.State(
    fresh = true,
    kidsSession = s?.kids == true,
    server = server.isNotEmpty(),
    inAccount = joined,
    profileStepDone = s?.profileStepDone == true,
    profiles = s?.profiles?.size ?: 1,
    pin = s?.profiles?.any { it.hasPin } == true,
    locked = s?.locked == true,
)

/** Who's watching?'s line after a first-run join: the provider, else the device that approved it (3.4). */
private fun whoLine(
    app: AppState,
    sync: SyncController,
): String? {
    if (!app.who) return null
    val by = sync.joinedBy
    val with = app.joinedWith
    return when {
        by != null -> {
            AccountRules.fill(FirstRunCopy.WHO_DEVICE, mapOf("device" to by))
        }

        with != null -> {
            AccountRules.fill(
                FirstRunCopy.WHO_PROVIDER,
                mapOf("provider" to AccountRules.providerName(with)),
            )
        }

        else -> {
            FirstRunCopy.WHO_SIGNED_IN
        }
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
 * [fresh]: an Incognito end the session has not read (`incognitoEnded`, also set for a scope found at launch): nothing
 * saved comes back, so no screen of that Incognito can open under the profile it returns to (plans/batch-10-incognito.md
 * 5.4). A stack saved under another profile never comes back here at all ([ProfileKeyed]).
 */
@Composable
internal fun rememberProfileStack(
    profile: String?,
    initial: List<Route>,
    seen: MutableSet<String?>,
    /** The phone has no Calendar tab: a stored one opens Library. */
    phone: Boolean = false,
    fresh: Boolean = false,
) = rememberNavBackStack(*initial.toTypedArray()).let { saved ->
    remember(saved) {
        if (seen.add(profile)) {
            val restored = saved.map { it as Route }
            val kept =
                if (fresh) initial else restoredStack(restored).let { if (phone) calendarInLibrary(it) else it }
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
    val incognito = ProfileCopy.inIncognito(gate.snapshot)
    val backStack =
        rememberProfileStack(
            gate.snapshot?.active,
            initial,
            app.stacksSeen,
            phone = form == Form.Phone,
            fresh = gate.snapshot?.incognitoEnded == true,
        )
    val routes = backStack.map { it as Route }
    val top = routes.last()
    val tv = form == Form.Tv
    // A kids session opens no Discover, Calendar, source, account or profile page (plans/m11-kids.md 5): every way
    // there is hidden, and a stack that holds one (saved, or pushed by a link) drops it.
    val kids = gate.snapshot?.kids == true || AccountRules.activeProfile(gate.snapshot)?.kids == true
    LaunchedEffect(kids, routes) {
        val kept = if (kids) kidsStack(routes) else routes
        if (kept != routes) {
            backStack.clear()
            backStack.addAll(kept)
        }
    }
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

    KindResetEffect(routes, app.versions::listEveryKind)

    fun choose(place: Route) {
        val next = switchedTo(routes, place)
        backStack.clear()
        backStack.addAll(next)
    }

    // Home's and Discover's Add an addon: Settings > Sources (TV's Settings opens on it).
    fun addAnAddon() {
        choose(Route.Settings)
        if (!tv) backStack.add(Route.SettingsSection(SettingsSection.Sources.name))
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

    val downloadsNav =
        DownloadsNav(
            onPlay = { backStack.add(Route.Player(it.kind, it.itemId, it.videoId, it.name)) },
            onVersion = { backStack.add(Route.Versions(it.kind, it.itemId, it.videoId, it.name, download = true)) },
            onSeries = { backStack.add(Route.DownloadedSeries(it)) },
        )
    // The first download asks for notifications once (plans/offline-downloads.md 3.3).
    if (!tv) NotificationAsk(app.downloads)
    // A download's "did not go through" belongs to the page it happened on (code review r2 n3).
    LaunchedEffect(top) { app.downloads.clearProblem() }

    // An addon's Install button (stremio://): the page with the link field, filled in, waits for Add.
    LaunchedEffect(marquee.addonLink) {
        val link = marquee.addonLink ?: return@LaunchedEffect
        marquee.addonLink = null
        app.linkInstall.offered = link
        if (backStack.lastOrNull() != Route.Sources) backStack.add(Route.Sources)
    }

    // First run (plans/first-screens.md 2): FirstRun.next over this device's state, on this stack. A join pulls the
    // account first ("Getting your profiles", 15 s at most), then opens Who's watching? or Home (D2). An account made
    // here goes on to the profile form.
    val firstRun = Route.Welcome in routes
    var created by rememberSaveable { mutableStateOf(false) }
    var pulling by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val nav =
        remember(backStack) {
            FirstRunNav(
                backStack,
                tv,
                state = { firstState(gate.snapshot, marquee.accounts.server(marquee), sync.joined) },
                onHome = ::arrive,
                onWho = {
                    arrive()
                    app.who = true
                    gate.ask()
                },
            )
        }

    fun joined() {
        if (pulling) return
        pulling = true
        scope.launch {
            // A problem from before the join (an earlier unreachable try) is not this pull's answer.
            val before = sync.problem
            withTimeoutOrNull(PULL_WAIT_MS) {
                snapshotFlow {
                    sync.lastSynced != null || (sync.problem != null && sync.problem !== before)
                }.first { it }
            }
            pulling = false
            nav.go(FirstRun.After.Joined)
        }
    }
    // A join met while a first-run page other than the sign-in shows (a pairing page, Account's kit): the same way on.
    // Not while a sign-in is under way: it says itself whether it made the account (onCreated) or joined one (onIn).
    val signInOpen = marquee.accounts.signIn != null || top is Route.SignIn
    LaunchedEffect(sync.joined, top, signInOpen) {
        if (joinGoesOn(sync.joined, firstRun, signInOpen, created)) joined()
    }
    // "Getting your profiles" holds Back too: nothing under it moves while the pull decides.
    BackHandler(enabled = pulling) {}
    // The wall over every first-run step and sign-in sheet, asked again at each while it has none; a setup opened
    // from an empty Home is a sheet with no wall (3.1: the wall is first run's and Sign in's only).
    val sheetTop = sheetTop(top, routes)
    val overWall = wallShows(top, routes, firstRun = app.start == Start.Onboarding)
    LaunchedEffect(top) { if (overWall) app.askWall() }

    // Sign in another device (the app shortcut, or a pairing code by link): the approve sheet, after the sign-in when
    // this device has no account yet. A TV approves nothing; a kids profile hears to ask a grown-up (the sheet says).
    val signingIn = routes.any { it is Route.SignIn }
    LaunchedEffect(marquee.pendingScan, sync.joined, signingIn) {
        when {
            !marquee.pendingScan || signingIn -> {
                Unit
            }

            tv -> {
                marquee.pendingScan = false
                marquee.pendingCode = null
            }

            sync.joined || AccountRules.activeProfile(gate.snapshot)?.kids == true -> {
                marquee.pendingScan = false
                if (backStack.lastOrNull() != Route.AddDevice) backStack.add(Route.AddDevice)
            }

            else -> {
                backStack.add(Route.SignIn(firstRun = firstRun))
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
        // Settings and one of its sections (the phone's drill-in) share every link.
        val settings: @Composable (String?) -> Unit = { section ->
            SettingsScreen(
                session,
                gate,
                art = settingsArt(app.home.rows, app.home.heroes, kids = gate.snapshot?.kids == true),
                onProfiles = { backStack.add(Route.Profiles) },
                onEditProfile = { backStack.add(Route.EditProfile(it)) },
                onManageProfiles = { backStack.add(Route.ManageProfiles) },
                sync,
                marquee.accounts,
                onSignIn = { backStack.add(if (tv) Route.TvSignIn else Route.SignIn(firstRun = false)) },
                onAddons = { backStack.add(Route.Addons) },
                onImport = { backStack.add(Route.Import) },
                onSync = { backStack.add(Route.Sync) },
                onDebrid = { backStack.add(Route.Debrid) },
                onTrakt = { backStack.add(Route.Trakt) },
                onHomeRows = { backStack.add(Route.HomeRows) },
                onFindAddons = { backStack.add(Route.FindAddons) },
                onStreamChoice = { backStack.add(Route.StreamChoice) },
                onDefaults = { backStack.add(Route.Defaults(DefaultsMode.Settings)) },
                onPlugins = { backStack.add(Route.Plugins) },
                section = section,
                onSection = { backStack.add(Route.SettingsSection(it)) },
            )
        }
        // The wall under the first-run pages, held still under every sheet, fading out with the last one.
        Box(Modifier.fillMaxSize()) {
            AnimatedVisibility(
                overWall,
                enter = fadeIn(tween(Marquee.panelMs)),
                exit = fadeOut(tween(Marquee.panelMs)),
            ) {
                FirstWall(app.wall, app.drift, sheet = top != Route.Welcome)
            }
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
                        entry<Route.Welcome> {
                            WelcomeScreen(
                                app.drift,
                                onGetStarted = { nav.go(FirstRun.After.GetStarted) },
                                onAlreadyUse =
                                    if (marquee.accounts
                                            .server(
                                                marquee,
                                            ).isEmpty()
                                    ) {
                                        null
                                    } else {
                                        ({ nav.go(FirstRun.After.AlreadyUse) })
                                    },
                            )
                        }
                        entry<Route.FirstProfile> {
                            FirstProfileSheet(
                                owner = gate.snapshot?.profiles?.firstOrNull { it.isOwner },
                                account = sync.joined,
                                busy = app.firstProfile.busy,
                                problem = app.firstProfile.problem,
                                onBack = if (backStack.size > 1) ::pop else null,
                                onContinue = { name, avatar ->
                                    app.firstProfile.finish(name, avatar) { nav.go(FirstRun.After.ProfileDone) }
                                },
                            )
                        }
                        entry<Route.Setup> {
                            SetupSheet(
                                app.setup,
                                AccountRules.activeProfile(gate.snapshot)?.name.orEmpty(),
                                rememberSetupChips(session),
                                onStart = { subtitle, chosen -> app.setup.start(subtitle, chosen, nav::setupDone) },
                                onSkip = { app.setup.skip { nav.setupDone(false) } },
                                onReview = { backStack.add(Route.Defaults(DefaultsMode.Review)) },
                            )
                        }
                        entry<Route.SignIn> { route ->
                            SignInScreen(
                                marquee.accounts,
                                session,
                                sync,
                                route.firstRun,
                                returning = route.returning,
                                onNotNow = {
                                    marquee.pendingScan = false
                                    if (route.firstRun) nav.go(FirstRun.After.NotNow) else pop()
                                },
                                onScan = { backStack.add(Route.Pairing) },
                                onKit = { backStack.add(Route.SyncJoin(kit = true)) },
                                onNoProviders = {
                                    pop()
                                    backStack.add(Route.Sync)
                                },
                                onCreated = { kit ->
                                    if (route.firstRun) {
                                        created = true
                                        nav.go(FirstRun.After.Created)
                                    } else {
                                        pop()
                                    }
                                    if (kit) backStack.add(Route.RecoveryKit)
                                },
                                onIn = { if (route.firstRun) joined() else pop() },
                                onBack = ::pop,
                                onProvider = { app.joinedWith = it },
                            )
                        }
                        entry<Route.TvSignIn> {
                            TvSignInScreen(
                                session,
                                sync,
                                marquee.accounts.server(marquee),
                                onNotNow = { if (firstRun) nav.go(FirstRun.After.NotNow) else pop() },
                                onOther = { backStack.add(Route.Sync) },
                                onIn = { if (firstRun) joined() else arrive() },
                            )
                        }
                        entry<Route.Import> {
                            // From setup: a sheet over the wall; Done or Skip import adds the defaults the import did not
                            // bring, then Home (3.5).
                            val first = Route.Setup in routes
                            ImportScreen(
                                session,
                                onDone = { if (first) app.setup.afterImport(::arrive) else arrive() },
                                onSkip = if (first) ({ app.setup.afterImport(::arrive) }) else null,
                                finishing = first && app.setup.busy,
                                failed = first && app.setup.failed != null,
                            )
                        }
                        entry<Route.Defaults> { route -> DefaultsScreen(session, route.mode, onDone = ::pop) }
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
                                // Incognito offers no recommended addons: the core refuses them there.
                                app.defaultsCard.check(
                                    kids = profile?.kids != false || incognito,
                                ) { defaultsSeen(session) }
                            }
                            // Home's one account card slot comes first (plans/accounts.md 4.3): a scheduled deletion
                            // is the one thing that must not wait behind the addons card.
                            // Right after a pairing, "Signed in from" the device that approved it, for 8 s: a line nothing
                            // focuses, so first focus and the tabs bar stay as they are (plans/tv-qr-login.md 5.5).
                            // A scheduled deletion still comes first.
                            val shownCard = marquee.accounts.homeCard()
                            val signedInFrom = sync.joinedBy.takeUnless { shownCard is AccountRules.Card.Deletion }
                            LaunchedEffect(signedInFrom) {
                                if (signedInFrom != null) {
                                    delay(8_000)
                                    sync.joinedBy = null
                                }
                            }
                            // A deletion, the account gone, a new device: first on Home, above the hero (the owner,
                            // 2026-11-02); "Signed in from" and the second way in stay after the first row.
                            val alertCard = shownCard?.takeIf { signedInFrom == null && AccountRules.urgent(it) }
                            val alert: (@Composable () -> Unit)? =
                                alertCard?.let { shown ->
                                    {
                                        AccountCard(
                                            marquee.accounts,
                                            sync,
                                            shown,
                                            onSignIn = {
                                                backStack.add(
                                                    if (tv) Route.TvSignIn else Route.SignIn(firstRun = false),
                                                )
                                            },
                                            onMakeKit = { backStack.add(Route.RecoveryKit) },
                                        )
                                    }
                                }
                            val accountCard: (@Composable () -> Unit)? =
                                signedInFrom?.let { by ->
                                    {
                                        Said(
                                            AccountRules.fill(AccountCopy.TV_SIGNED_IN_FROM, mapOf("device" to by)),
                                            Status.Ok,
                                        )
                                    }
                                } ?: shownCard?.takeIf { alertCard == null }?.let { shown ->
                                    {
                                        AccountCard(
                                            marquee.accounts,
                                            sync,
                                            shown,
                                            onSignIn = {
                                                backStack.add(
                                                    if (tv) Route.TvSignIn else Route.SignIn(firstRun = false),
                                                )
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
                            // An Incognito end this session has not read: first, until its OK (plans/batch-10-incognito.md 5.4).
                            val endedCard: (@Composable () -> Unit)? =
                                if (profile?.incognitoEnded == true) ({ IncognitoEndedCard(gate::ackEnded) }) else null
                            // Both show, the account card first, as on the web.
                            val cards = listOfNotNull(endedCard, accountCard, addonsCard)
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
                                if (removal.removing != null ||
                                    removal.failed != null
                                ) {
                                    ({ RemovalNote(removal) })
                                } else {
                                    null
                                }
                            if (tv) {
                                TvHome(
                                    app.home,
                                    app.tvHome,
                                    onOpen = ::open,
                                    onPlay = ::playTile,
                                    onSetup = {
                                        app.setup.reset()
                                        backStack.add(Route.Setup)
                                    },
                                    onAddons = ::addAnAddon,
                                    rowsHidden = rowsHidden,
                                    onHomeRows = onHomeRows,
                                    kids = kids,
                                    alert = alert,
                                    card = card,
                                    removal = app.removal,
                                )
                            } else {
                                PhoneHome(
                                    app.home,
                                    onOpen = ::open,
                                    onPlay = ::playTile,
                                    onSetup = {
                                        app.setup.reset()
                                        backStack.add(Route.Setup)
                                    },
                                    onAddons = ::addAnAddon,
                                    rowsHidden = rowsHidden,
                                    onHomeRows = onHomeRows,
                                    kids = kids,
                                    alert = alert,
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
                                kids = kids,
                            )
                        }
                        entry<Route.Discover> {
                            DiscoverScreen(
                                session,
                                ::open,
                                request = app.discoverRequest,
                                onRequestSent = { app.discoverRequest = null },
                                onSetup = {
                                    app.setup.reset()
                                    backStack.add(Route.Setup)
                                },
                                onAddons = ::addAnAddon,
                                focus = app.discoverFocus,
                            )
                        }
                        entry<Route.Library> {
                            // On the phone Calendar is a row at the top of Library, pushed over it (D6).
                            LibraryScreen(
                                session,
                                ::open,
                                onCalendar = if (tv || kids) null else ({ backStack.add(Route.Calendar) }),
                                onDiscover = if (kids) null else ({ choose(Route.Discover) }),
                                onDownloads = if (tv) null else ({ backStack.add(Route.Downloads) }),
                            )
                        }
                        entry<Route.Downloads> {
                            DownloadsScreen(session, app.downloads, downloadsNav, kids = gate.snapshot?.kids != false)
                        }
                        entry<Route.DownloadedSeries> { route ->
                            DownloadedSeriesScreen(
                                session,
                                route.itemId,
                                app.downloads,
                                downloadsNav,
                                kids = gate.snapshot?.kids != false,
                                onGone = { if (backStack.lastOrNull() == route) pop() },
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
                        entry<Route.Settings> { settings(null) }
                        entry<Route.SettingsSection> { route -> settings(route.id) }
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
                            FindAddonsScreen(session, app.installCards, app.linkInstall, onPlugins = {
                                backStack.add(Route.PluginDirectory)
                            }, onPasteLink = {
                                if (fromSources) pop() else backStack.add(Route.Sources)
                            })
                        }
                        entry<Route.Debrid> { DebridScreen(session) }
                        entry<Route.Plugins> { PluginsScreen(session) }
                        entry<Route.PluginDirectory> { PluginDirectoryScreen(session) }
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
                                    // First run's other device: the join pulls, then Who's watching? or Home.
                                    firstRun -> {
                                        joined()
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
                            AddDeviceScreen(
                                sync,
                                marquee.accounts.approval,
                                approves = marquee.accounts.rows(tv).approve,
                                linked = marquee.pendingCode,
                                onTaken = { marquee.pendingCode = null },
                                onDone = ::pop,
                            )
                        }
                        entry<Route.RecoveryKit> { RecoveryKitScreen(sync, onDone = ::pop) }
                        entry<Route.ReportLost> { route -> ReportLostScreen(sync, route.id, onDone = ::pop) }
                        entry<Route.Title> { route ->
                            TitleScreen(
                                route,
                                session,
                                app,
                                form,
                                onBack = ::pop,
                                onPlay = { key, videoId, title ->
                                    backStack.add(Route.Player(route.kind, route.id, videoId, title, key))
                                },
                                onStartOver = { key, videoId, title ->
                                    backStack.add(
                                        Route.Player(route.kind, route.id, videoId, title, key, startOver = true),
                                    )
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
                                canDownload = !tv && gate.snapshot?.kids == false,
                                onDownloadVersion = { videoId, name ->
                                    backStack.add(Route.Versions(route.kind, route.id, videoId, name, download = true))
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
                                // A kids profile adds no source: no Settings to a page it cannot open.
                                onAddSource = if (kids) null else ({ backStack.add(Route.Sources) }),
                                onBack = ::pop,
                                onUndo = { session.ops.undoLearned(it.screenClass, it.choice) },
                                // Over the paused player its attempt keeps resolving: the rows hold their order.
                                held = backStack.getOrNull(backStack.indexOf(route) - 1) is Route.Player,
                            )
                        }
                        entry<Route.Player> { route ->
                            // Removed from Continue Watching a moment ago and played now: the removal waiting for Undo goes.
                            LaunchedEffect(route) { app.removal.played(route.id) }
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
                                            backStack.add(
                                                Route.Versions(route.kind, route.id, route.videoId, route.title),
                                            )
                                        }
                                    },
                                    onTitle =
                                        if (Format.parseVideo(route.videoId) !=
                                            null
                                        ) {
                                            ({ toTitle(route) })
                                        } else {
                                            null
                                        },
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
                                    // Another version mid-play goes on from where it was, never from the start again.
                                    onOffer = { key -> replacePlayer(route, route.copy(key = key, startOver = false)) },
                                    // Where an addon is added (noSource). Accounts are set up in Settings only.
                                    onSettings = if (kids) null else ({ backStack.add(Route.Sources) }),
                                )
                            }
                        }
                    },
            )
            if (pulling) GettingProfiles()
        }
    }

    val selected = routes.lastOrNull { it.isTopLevel } ?: Route.Home
    val framed = top.isTopLevel

    // The profile's menu (plans/nav-profile.md): a profile with a PIN opens the picker on its keypad.
    fun pinFor(id: String) {
        gate.asked = id
        backStack.add(Route.Profiles)
    }
    var profileMenu by remember { mutableStateOf(false) }
    val me = AccountRules.activeProfile(gate.snapshot)
    val phoneProfile =
        me?.let {
            // In Incognito the tab says so under its mark (plans/batch-10-incognito.md 5.2).
            val label = if (it.id == ProfileCopy.INCOGNITO) it.name else PhoneProfileTab.LABEL
            PhoneProfileTab("Profile: ${it.name}", { profileMenu = true }, label) { ProfileAvatar(it, 26.dp) }
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
            kids = kids,
            content = content,
        )
    } else {
        PhoneShell(
            selected = selected,
            showTabs = framed,
            onSelect = ::choose,
            statusStrip = top !is Route.Title && top !is Route.Player && !sheetTop,
            insetSides = top !is Route.Player && !sheetTop,
            onBack = if (top.hasBackLink) ({ systemBack?.onBackPressed() ?: pop() }) else null,
            profile = phoneProfile,
            // Offline, every page but the player says so, one press from the downloads (plans/offline-downloads.md 3.5).
            banner =
                offlineBanner(marquee.offline, marquee.downloadKeeper.anyDone, top)?.let { toDownloads ->
                    {
                        OfflineBanner(
                            onDownloads =
                                if (toDownloads) {
                                    {
                                        choose(Route.Library)
                                        backStack.add(Route.Downloads)
                                    }
                                } else {
                                    null
                                },
                        )
                    }
                },
            kids = kids,
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

/**
 * After Remove from Continue Watching: the words with Undo while it can be taken back, or why it failed. Under the row
 * on Home; on the title page [undoFocus] takes the focus to Undo, as the button that was pressed has gone.
 */
@Composable
internal fun RemovalNote(
    removal: PendingRemoval,
    modifier: Modifier = Modifier.padding(horizontal = Marquee.safe.side),
    undoFocus: FocusRequester? = null,
    onUndo: () -> Unit = removal::undo,
) {
    val tile = removal.removing
    val words = tile?.let { TileCopy.removed(it.name) } ?: removal.failed ?: return
    if (undoFocus != null) {
        LaunchedEffect(tile?.id) {
            if (tile == null) return@LaunchedEffect
            // Attached on the next frame, not during this composition.
            withFrameNanos {}
            undoFocus.requestFocus()
        }
    }
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(
            words,
            role = TextRole.Callout,
            color = Marquee.colors.muted,
            modifier = Modifier.weight(1f, fill = false).semantics { liveRegion = LiveRegionMode.Polite },
        )
        if (tile != null) SecondaryButton("Undo", onUndo, focusRequester = undoFocus)
    }
}

/** Whether a join met on first run goes on by the rule: not during a sign-in, nor after this device made the account. */
internal fun joinGoesOn(
    joined: Boolean,
    firstRun: Boolean,
    signingIn: Boolean,
    created: Boolean,
) = joined && firstRun && !signingIn && !created

/** Whether the wall shows under [top]: a sheet page while the start is first run, and any sign-in sheet (3.1). */
internal fun wallShows(
    top: Route,
    routes: List<Route>,
    firstRun: Boolean,
) = sheetTop(top, routes) && (firstRun || top is Route.SignIn || top == Route.TvSignIn)

/** Whether [top] is a sheet page: a first-run step, any sign-in sheet (D10), or Import from setup. */
internal fun sheetTop(
    top: Route,
    routes: List<Route>,
): Boolean =
    top == Route.Welcome || top == Route.FirstProfile || top == Route.Setup || top is Route.SignIn ||
        top == Route.TvSignIn || (top == Route.Import && Route.Setup in routes)

/** How long a first-run join waits for its first pull before it decides on the profiles it has. */
private const val PULL_WAIT_MS = 15_000L

/**
 * The shell's offline banner on page [top] (plans/offline-downloads.md 3.5): null for none (online, or the player),
 * else whether it leads to the downloads (the profile has one, and this is not a Downloads page).
 */
internal fun offlineBanner(
    offline: Boolean,
    anyDone: Boolean,
    top: Route,
): Boolean? =
    when {
        !offline || top is Route.Player -> null
        else -> anyDone && top != Route.Downloads && top !is Route.DownloadedSeries
    }

/**
 * A kind chosen on Versions holds while that video's Versions page or player is on the stack; once the video leaves
 * (back to its title, a player ending on the title, any other place), every kind is listed again, so the title's Play
 * plays the core's pick (plans/plugins-and-marketplace.md 4.2).
 */
@Composable
internal fun KindResetEffect(
    routes: List<Route>,
    listEveryKind: (videoId: String) -> Unit,
) {
    val choosing = choosingVideos(routes)
    var choosingBefore by remember { mutableStateOf(choosing) }
    LaunchedEffect(choosing) {
        for (gone in choosingBefore - choosing) listEveryKind(gone)
        choosingBefore = choosing
    }
}
