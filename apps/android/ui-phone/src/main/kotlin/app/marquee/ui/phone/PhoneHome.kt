package app.marquee.ui.phone

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.currentStateAsState
import app.marquee.core.KidsCopy
import app.marquee.core.TileCopy
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.RowNotice
import app.marquee.core.model.Tile
import app.marquee.core.model.allFailed
import app.marquee.core.model.notice
import app.marquee.core.model.playTile
import app.marquee.core.model.tileKeys
import app.marquee.core.model.withoutRemoved
import app.marquee.core.views.HomeView
import app.marquee.core.views.ViewStatus
import app.marquee.design.ActionMenu
import app.marquee.design.Backdrop
import app.marquee.design.HeroClock
import app.marquee.design.HeroDots
import app.marquee.design.HeroTrailers
import app.marquee.design.HoldTrailerWhileScrolling
import app.marquee.design.Icons
import app.marquee.design.LocalReducedMotion
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.ProvideCaptionMeasurer
import app.marquee.design.RatingsRow
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.RunHeroClock
import app.marquee.design.Scrims
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.Tokens
import app.marquee.design.TrailerHost
import app.marquee.design.TrailerPreviews
import app.marquee.design.TrailerState
import app.marquee.design.TrailerSurface
import app.marquee.design.WarmTrailerPage
import app.marquee.design.describe
import app.marquee.design.heroClockRuns
import app.marquee.design.heroNearestPage
import app.marquee.design.heroPagerCount
import app.marquee.design.heroPagerPage
import app.marquee.design.laneHeight
import app.marquee.design.rememberScreenReader
import app.marquee.design.rememberTrailerHost
import app.marquee.design.stopOnDrag
import app.marquee.design.trailerAllowedLogged
import app.marquee.design.windowDp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Home on a phone, as the iPhone TV app (design/DECISION.md, Home and Shelves): the 390 by 680 hero full bleed
 * under the status bar, then one swipeable lane per catalog: Continue Watching as 16:9 cards with their line,
 * the catalogs as bare 99 by 149 posters.
 */
@Composable
fun PhoneHome(
    home: HomeView,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    modifier: Modifier = Modifier,
    /** Catalogs exist and every one is hidden: the empty state points to Home rows, not setup. */
    rowsHidden: Boolean = false,
    onHomeRows: () -> Unit = {},
    /** A kids session: its empty Home asks for a grown-up, never for addons (plans/m11-kids.md 6.4). */
    kids: Boolean = false,
    /**
     * An urgent account alert (a deletion, the account gone, a new device): first, above the hero, under the status
     * bar (the owner, 2026-11-02). It lies over the hero's art, so one that lands after Home opened moves nothing
     * (QA r2, Q2 row 33).
     */
    alert: (@Composable () -> Unit)? = null,
    /** A one-time card (new recommended addons): under Continue Watching, or first when there is none. */
    card: (@Composable () -> Unit)? = null,
    /** "Remove from Continue Watching", from a press and hold on one of its tiles; null offers no menu. */
    onRemove: ((Tile) -> Unit)? = null,
    /** The title of a removal waiting for its Undo: left out of Continue Watching meanwhile. */
    removing: String? = null,
    /** The removal's note (Undo, or why it failed), under Continue Watching. */
    notice: (@Composable () -> Unit)? = null,
) {
    // A snapshot that came while this page first drew reaches every reader (LiveView.follow).
    home.follow()
    var menuFor by remember { mutableStateOf<Tile?>(null) }
    menuFor?.let { tile ->
        ActionMenu(
            title = tile.name,
            actions = listOf(MenuAction(TileCopy.REMOVE_CONTINUE, Icons.X) { onRemove?.invoke(tile) }),
            onDismiss = { menuFor = null },
        )
    }
    val hold: ((Tile) -> Unit)? = onRemove?.let { { tile -> menuFor = tile } }
    val listState = rememberLazyListState()
    val hasHero = home.hero != null
    // Home's trailers, above the list, so a scroll never makes a new host; every host plays on the app's one player
    // page (plans/appletv-polish.md 4.2, batch 13 perf).
    val trailers = rememberTrailerHost()
    HoldTrailerWhileScrolling(trailers) { listState.isScrollInProgress }
    WarmTrailerPage(trailers, home.heroes.any { HeroTrailers.checkedId(it.trailer) != null })
    // At least half the hero on screen: the carousel's clock and its trailer run only then.
    val heroVisible by remember(listState) {
        derivedStateOf {
            val item = listState.layoutInfo.visibleItemsInfo.firstOrNull { it.key == "hero" }
            item != null && item.size > 0 && (item.size + minOf(0, item.offset)) * 2 >= item.size
        }
    }
    // The list keeps the first lane where it is when a hero arrives above it; someone who has not
    // scrolled yet should see the hero.
    // An alert that lands above the hero is seen the same way, by someone who has not scrolled yet.
    val hasAlert = alert != null
    LaunchedEffect(hasHero, hasAlert) {
        if ((hasHero || hasAlert) && listState.firstVisibleItemIndex <= 1 &&
            listState.firstVisibleItemScrollOffset == 0
        ) {
            listState.scrollToItem(0)
        }
    }
    // The hero runs up under the status bar; without one the page starts below it.
    val top = if (hasHero) 0.dp else WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + Marquee.safe.top
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(top = top, bottom = Marquee.space.l + LocalBarSpace.current),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        val hero = home.hero
        // Without a hero the alert is the page's first item.
        if (alert != null && hero == null) item(key = "alert") { alert() }
        if (hero !=
            null
        ) {
            item(key = "hero") {
                Box {
                    PhoneHeroCarousel(
                        home.heroes,
                        onOpen,
                        onPlay,
                        trailers,
                        titleHidden = alert != null,
                    ) { heroVisible }
                    // Over the hero's top, under the status bar, where the page title was.
                    if (alert != null) {
                        Box(
                            Modifier.windowInsetsPadding(WindowInsets.statusBars).padding(top = Marquee.space.s),
                        ) { alert() }
                    }
                }
            }
        }
        if (home.rows.isEmpty()) {
            item(
                key = "state",
            ) { HomeState(home, onSetup, onAddons, rowsHidden, onHomeRows, kids) }
        }
        if (hero == null && home.rows.toList().allFailed()) {
            item(key = "failed") { AllFailed(home, Modifier.padding(horizontal = Marquee.safe.side)) }
        } else {
            val rows = home.rows.toList().withoutRemoved(removing)
            val split = rows.indexOfFirst { it.rowId == TileCopy.CONTINUE_ROW } + 1
            items(rows.subList(0, split), key = {
                it.rowId
            }) { row -> PhoneRow(row, onOpen, hold, home::retryRow) }
            if (notice != null) item(key = "notice") { notice() }
            if (card != null) item(key = "card") { card() }
            items(
                rows.subList(split, rows.size),
                key = { it.rowId },
            ) { row -> PhoneRow(row, onOpen, hold, home::retryRow) }
        }
    }
}

@Composable
private fun HomeState(
    home: HomeView,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    rowsHidden: Boolean,
    onHomeRows: () -> Unit,
    kids: Boolean,
) {
    val side = Marquee.safe.side
    when (val status = home.status) {
        is ViewStatus.Failed -> {
            StateMessage(
                title = "Home could not load",
                body = status.reason,
                icon = Icons.CircleAlert,
                actionLabel = "Try again",
                onAction = home::retry,
                modifier = Modifier.padding(horizontal = side),
            )
        }

        ViewStatus.Opening -> {
            SkeletonLane(TileShape.Poster)
        }

        ViewStatus.Open -> {
            if (kids) {
                StateMessage(KidsCopy.HOME_EMPTY_TITLE, KidsCopy.HOME_EMPTY, Modifier.padding(horizontal = side))
                return
            }
            if (rowsHidden) {
                // Catalogs exist and every one is hidden: setup would change nothing.
                StateMessage(
                    title = "Every Home row is hidden",
                    body =
                        "Your addons have catalogs, and each one is hidden from Home. " +
                            "Show them again in Home rows.",
                    actionLabel = "Show rows",
                    onAction = onHomeRows,
                    modifier = Modifier.padding(horizontal = side),
                )
                return
            }
            // Open with no rows: no addon that is on has a catalog, or every row is hidden.
            StateMessage(
                title = "Home fills up from your addons",
                body =
                    "None of the addons you have on shows a catalog here. " +
                        "Set up Momotus offers the recommended addons in one step, or add an addon yourself.",
                actionLabel = "Set up Momotus",
                onAction = onSetup,
                secondaryLabel = "Add an addon",
                onSecondary = onAddons,
                modifier = Modifier.padding(horizontal = side),
            )
        }
    }
}

/** Every lane failed and none has a tile (most often no network): one way on instead of a column of failures. */
@Composable
private fun AllFailed(
    home: HomeView,
    modifier: Modifier = Modifier,
) {
    StateMessage(
        title = "Home could not load",
        body = "None of your addons answered. Check the network, then try again.",
        icon = Icons.CircleAlert,
        actionLabel = "Try again",
        onAction = home::retry,
        modifier = modifier,
    )
}

/**
 * The hero carousel (plans/appletv-polish.md 4.1, 4.2): the core's pages side by side, swiped by hand or paged by
 * the clock on input idle, the dots in the hero's foot, the large title and the sound switch fixed over them. The
 * page shown plays its trailer under its art, which fades once YouTube's start controls have gone.
 */
@Composable
private fun PhoneHeroCarousel(
    heroes: List<Hero>,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
    trailers: TrailerHost,
    /** An alert lies over the page title: the title gives way (unseen, still the heading to TalkBack). */
    titleHidden: Boolean = false,
    visible: () -> Boolean,
) {
    val n = heroes.size
    // Many loops of the pages: the timer always moves right, past the last page too (review r1, L3).
    val pager = rememberPagerState(initialPage = heroPagerPage(n, 0)) { heroPagerCount(n) }
    val clock = remember { HeroClock(heroes.size) }
    val shown = { page: Int -> heroes[page % n] }
    // The list grows while catalogs arrive: the page shown stays the same title.
    var shownId by remember { mutableStateOf<String?>(null) }
    SideEffect { shownId = heroes.getOrNull(pager.currentPage % n.coerceAtLeast(1))?.tile?.id }
    LaunchedEffect(heroes.map { it.tile.id }) {
        val at = heroes.indexOfFirst { it.tile.id == shownId }.coerceAtLeast(0)
        // Also out of the first loop, where a list that began with one page started: a swipe back from the first
        // page then reaches the last (review r2, N1).
        if (n > 0 &&
            (pager.currentPage % n != at || (n > 1 && pager.currentPage < n))
        ) {
            pager.scrollToPage(heroPagerPage(n, at))
        }
    }
    val screenReader = rememberScreenReader()
    SideEffect { clock.count = heroes.size }
    val trailer = remember { TrailerState() }
    val context = LocalContext.current
    val reduced = LocalReducedMotion.current
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val resumed by lifecycle.currentStateAsState()
    val active = resumed.isAtLeast(Lifecycle.State.RESUMED)
    // A drag by hand: no more timed pages this session.
    LaunchedEffect(pager) {
        clock.stopOnDrag(pager.interactionSource)
    }
    RunHeroClock(
        clock,
        running = {
            heroClockRuns(
                active,
                visible(),
                reduced,
                pager.isScrollInProgress,
                trailer.playing,
                heroes.size,
                screenReader = screenReader,
            )
        },
    ) {
        scope.launch {
            pager.animateScrollToPage(
                pager.currentPage + 1,
                animationSpec = tween(HeroClock.SLIDE_MS, easing = Tokens.ease),
            )
        }
    }
    LaunchedEffect(trailer) { snapshotFlow { trailer.fraction }.collect { if (trailer.playing) clock.trailer(it) } }
    // The settled page's trailer, once it is due and every condition holds (decision 5).
    val settledId = heroes.getOrNull(pager.settledPage % n.coerceAtLeast(1))?.tile?.id
    LaunchedEffect(settledId, active, visible()) {
        trailers.stop()
        val page = heroes.getOrNull(pager.settledPage % n.coerceAtLeast(1)) ?: return@LaunchedEffect
        val id = HeroTrailers.checkedId(page.trailer)
        if (!active || !visible() || id == null) return@LaunchedEffect
        delay(TRAILER_DELAY_MS)
        val allowed =
            trailerAllowedLogged(
                TrailerPreviews.signals(
                    context,
                    visible = true,
                    reducedMotion = reduced,
                    tv = false,
                    hasTrailer =
                        page.tile.id !in spentTrailers,
                ),
            )
        if (!allowed) return@LaunchedEffect
        trailers.play(id, trailer) { why ->
            if (why != "stopped") spentTrailers += page.tile.id
            // Played to its cap: the art fades back, and the same clock pages on 600 ms later.
            if (why == "cap") clock.trailer(1f - 600f / HeroClock.DWELL_MS)
        }
    }
    DisposableEffect(Unit) { onDispose { trailers.stop() } }
    val art by animateFloatAsState(if (trailer.shown) 0f else 1f, tween(600, easing = Tokens.ease), label = "heroArt")
    Box(
        Modifier.pointerInput(Unit) {
            // Any touch starts the dwell over; the gesture still goes to the pager and the list.
            awaitPointerEventScope {
                while (true) {
                    awaitPointerEvent(PointerEventPass.Initial)
                    clock.input()
                }
            }
        },
    ) {
        HorizontalPager(state = pager, beyondViewportPageCount = 1) { page ->
            val on = page == pager.settledPage
            PhoneHero(
                shown(page),
                onOpen,
                onPlay,
                artAlpha = { if (on) art else 1f },
                under = if (on && trailer.loading) ({ TrailerSurface(trailers, Modifier.matchParentSize()) }) else null,
                lag = { pager.getOffsetDistanceInPages(page) },
            )
        }
        Row(
            Modifier
                .fillMaxWidth()
                .windowInsetsPadding(WindowInsets.statusBars)
                .padding(start = 16.dp, end = 16.dp, top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqText(
                "Home",
                Modifier.weight(1f).alpha(if (titleHidden) 0f else 1f).semantics { heading() },
                role = TextRole.Hero,
                maxLines = 1,
            )
        }
        if (heroes.size > 1) {
            HeroDots(
                heroes.size,
                pager.currentPage % n,
                { clock.progress },
                Modifier.align(Alignment.BottomCenter).padding(bottom = 5.dp),
                label = { "Page ${it + 1} of ${heroes.size}, ${heroes[it].tile.name}" },
                onPick = { i ->
                    clock.manual()
                    scope.launch {
                        pager.animateScrollToPage(
                            heroNearestPage(n, pager.currentPage, i),
                            animationSpec = tween(HeroClock.SLIDE_MS, easing = Tokens.ease),
                        )
                    }
                },
            )
        }
        // The trailer's sound, at the hero's foot level with the dots, as the TV app's (plans/appletv-polish.md 18):
        // drawn from the moment a trailer plays, when a kept sound starts too (review r4, W2), under one name, on or off.
        if (trailer.playing && (!HeroTrailers.walk || HeroTrailers.walkSwitch)) {
            TrailerSoundSwitch(trailers, Modifier.align(Alignment.BottomEnd).padding(end = 16.dp, bottom = 5.dp))
        }
    }
}

/** Pages whose trailer ended, failed or was cut this session: they do not play again. */
private val spentTrailers = mutableSetOf<String>()

/** The page settles, then the player starts loading under its art. */
private const val TRAILER_DELAY_MS = 700L

/**
 * One page of the iPhone's hero (the home-phone frame): the art full bleed, 390 by 680 on a 390 wide phone, and a
 * centered column at its foot: the logo, the kind and genres with the ratings row, the core's reason, then the
 * 178 by 48 Play pill and the 48 round Details. A tap on the art opens the details too. The trailer, when it plays,
 * sits [under] the art, whose alpha is [artAlpha].
 */
@Composable
private fun PhoneHero(
    hero: Hero,
    onOpen: (Tile) -> Unit,
    onPlay: (Tile) -> Unit,
    artAlpha: () -> Float = { 1f },
    under: (@Composable BoxScope.() -> Unit)? = null,
    lag: (() -> Float)? = null,
) {
    val tile = hero.tile
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        // The frame's shape, held to most of a short window (a landscape phone) so Play still shows.
        val height = minOf(maxWidth * HERO_RATIO, windowDp().height * HERO_WINDOW)
        Box(
            Modifier
                .fillMaxWidth()
                .height(height)
                .testTag(PHONE_HERO_TAG)
                .semantics {
                    // The buttons on the hero are hidden from screen readers, so Play is an action of the hero, which
                    // itself opens the details.
                    hero.playTile()?.let { start ->
                        customActions =
                            listOf(
                                CustomAccessibilityAction("${hero.play?.label} ${tile.name}") {
                                    onPlay(start).let { true }
                                },
                            )
                    }
                }.clickable(role = Role.Button, onClickLabel = "Details of ${tile.name}") { onOpen(tile) },
        ) {
            Backdrop(
                TileCopy.art(tile, wide = true).first,
                Modifier.matchParentSize(),
                foot = Scrims.phoneHomeFoot,
                top = Scrims.phoneHomeTop,
                alignment = if (tile.background == null) Alignment.TopCenter else Alignment.Center,
                artAlpha = artAlpha,
                trailer = under,
                lag = lag,
            )
            Column(
                Modifier
                    .align(Alignment.BottomCenter)
                    .padding(start = Marquee.safe.side, end = Marquee.safe.side, bottom = HERO_FOOT),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Logo(
                    tile.logo,
                    tile.name,
                    height = 86.dp,
                    maxWidth = 300.dp,
                    role = TextRole.Title1,
                    color = Marquee.colors.text,
                )
                FlowRow(
                    Modifier.padding(top = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    itemVerticalAlignment = Alignment.CenterVertically,
                ) {
                    MqText(
                        TileCopy.heroGenres(hero),
                        role = TextRole.Callout,
                        weight = FontWeight.SemiBold,
                        maxLines = 1,
                    )
                    RatingsRow(TileCopy.ratings(tile.ratings), onArt = true)
                }
                // The core's reason on Apple's explainability line; never an eyebrow.
                if (hero.reason.isNotBlank()) {
                    MqText(
                        hero.reason,
                        Modifier.padding(top = 6.dp),
                        role = TextRole.Caption,
                        weight = FontWeight.SemiBold,
                        maxLines = 2,
                        textAlign = TextAlign.Center,
                    )
                }
                // The hero is the control for screen readers (Details, and Play as its action); the buttons repeat
                // it for sighted touch.
                Row(
                    Modifier.padding(top = 18.dp).clearAndSetSemantics { testTag = PHONE_HERO_BUTTONS },
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val play = hero.play
                    if (play != null) {
                        PrimaryButton(
                            label = play.label,
                            icon = PlayFilled,
                            onClick = { onPlay(tile.copy(videoId = play.videoId)) },
                            modifier = Modifier.width(178.dp).describe("${play.label} ${tile.name}"),
                        )
                    } else if (hero.playPending) {
                        // Play's place while the core reads the page's episodes: Details never moves when it lands.
                        Box(Modifier.alpha(0f)) {
                            PrimaryButton("Play", {}, Modifier.width(178.dp), icon = PlayFilled, enabled = false)
                        }
                    }
                    RoundButton(Icons.Info, "Details", { onOpen(tile) })
                }
            }
        }
    }
}

/** The trailer's sound switch: one name, on or off, showing the session's choice ([HeroTrailers.sound]). */
@Composable
internal fun TrailerSoundSwitch(
    trailers: TrailerHost,
    modifier: Modifier = Modifier,
) {
    val sound = HeroTrailers.sound
    RoundButton(
        if (sound) Icons.Volume2 else Icons.VolumeX,
        SOUND_LABEL,
        { trailers.sound(!sound) },
        modifier.testTag(PHONE_HERO_SOUND),
        selected = sound,
        toggle = true,
    )
}

/** The hero and its button row, for tests of where they sit. */
internal const val PHONE_HERO_TAG = "home-hero"

/** The trailer's sound switch, and its one name. */
internal const val PHONE_HERO_SOUND = "home-hero-sound"
internal const val SOUND_LABEL = "Trailer sound"
internal const val PHONE_HERO_BUTTONS = "home-hero-buttons"

/** The frame's 390 by 680. */
private const val HERO_RATIO = 680f / 390f

/** The most of the window's height a hero takes. */
private const val HERO_WINDOW = 0.85f

/** The buttons' foot over the hero's: the frame's 56, the dots in it. */
private val HERO_FOOT = 56.dp

/** Continue Watching is a lane of 16:9 stills; every catalog row is posters, as Stremio shows them. */
private fun shapeOf(row: HomeRow): TileShape =
    when {
        TileCopy.isContinue(row.rowId) -> TileShape.Landscape
        else -> TileShape.Poster
    }

/** A catalog's posters are the art alone on the phone; Continue Watching keeps its name and line. */
private fun captioned(row: HomeRow): Boolean = TileCopy.isContinue(row.rowId)

@Composable
private fun PhoneRow(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
    hold: ((Tile) -> Unit)?,
    /** A failed row's Try again: that catalog only. */
    onRetry: (String) -> Unit,
) {
    val side = Marquee.safe.side
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        // The catalog's name and kind alone, never its addon (design/DECISION.md, Shelves).
        RowHeader(row.title, null, Modifier.padding(horizontal = side))
        when (val notice = row.notice()) {
            is RowNotice.Waiting -> {
                SkeletonLane(shapeOf(row), captioned(row))
                val detail = notice.detail
                if (detail != null) {
                    MqText(
                        detail,
                        role = TextRole.Caption,
                        color = Marquee.colors.muted,
                        modifier = Modifier.padding(horizontal = side),
                    )
                }
            }

            is RowNotice.Explained -> {
                StateMessage(
                    title = notice.title,
                    body = notice.body,
                    icon = if (notice.failed) Icons.CircleAlert else null,
                    actionLabel = "Try again".takeIf { notice.failed },
                    onAction = { onRetry(row.rowId) }.takeIf { notice.failed },
                    modifier = Modifier.padding(horizontal = side),
                )
            }

            null -> {
                TileLane(row, onOpen, hold?.takeIf { TileCopy.removable(row.rowId) })
            }
        }
    }
}

@Composable
private fun SkeletonLane(
    shape: TileShape,
    captioned: Boolean = true,
) {
    val metrics = Marquee.tiles
    val size =
        if (shape ==
            TileShape.Poster
        ) {
            metrics.poster
        } else if (shape == TileShape.Landscape) {
            metrics.landscape
        } else {
            metrics.square
        }
    Row(
        Modifier
            .height(
                Marquee.tiles.laneHeight(shape, captioned),
            ).padding(start = Marquee.safe.side, top = metrics.pad),
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
    ) {
        repeat(SKELETON_TILES) { Skeleton(Modifier.size(size.width, size.height)) }
    }
}

private const val SKELETON_TILES = 4

/** The genre line under a Top 10 poster. */
private val RANK_GENRE = 22.dp

@Composable
private fun TileLane(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
    onHold: ((Tile) -> Unit)?,
) {
    val metrics = Marquee.tiles
    val shape = shapeOf(row)
    val keys = remember(row.tiles) { row.tileKeys() }
    val captioned = captioned(row)
    // One caption measurer for the lane's tiles, not two per tile.
    ProvideCaptionMeasurer {
        LazyRow(
            // A Top 10 row gives each poster its genre, one line under the art.
            modifier =
                Modifier.height(
                    Marquee.tiles.laneHeight(shape, captioned) + if (row.ranked && !captioned) RANK_GENRE else 0.dp,
                ),
            contentPadding = PaddingValues(horizontal = Marquee.safe.side),
            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        ) {
            itemsIndexed(row.tiles, key = { index, _ -> keys[index] }) { _, tile ->
                val (art, fallback) = TileCopy.art(tile, wide = shape == TileShape.Landscape)
                PosterTile(
                    title = tile.name,
                    imageUrl = art,
                    fallbackImageUrl = fallback,
                    onClick = { onOpen(tile) },
                    modifier = Modifier.padding(top = metrics.pad),
                    shape = shape,
                    progress = tile.progress,
                    caption = TileCopy.caption(row.rowId, tile) ?: tile.releaseInfo,
                    // Continue Watching carries the core's line alone: no scores, no badges.
                    captioned = captioned,
                    artAlignment =
                        if (shape == TileShape.Landscape &&
                            tile.background == null
                        ) {
                            Alignment.TopCenter
                        } else {
                            Alignment.Center
                        },
                    onLongClick = onHold?.let { { it(tile) } },
                    longClickLabel = "More options".takeIf { onHold != null },
                    rank = tile.rank,
                    genre = tile.genre,
                )
            }
        }
    }
}
