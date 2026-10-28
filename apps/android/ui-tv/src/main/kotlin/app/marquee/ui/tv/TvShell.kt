@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import app.marquee.core.route.Route
import app.marquee.core.route.TOP_LEVEL
import app.marquee.design.BrandMark
import app.marquee.design.Icons
import app.marquee.design.KeyPresses
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.Tokens
import app.marquee.design.focusPalette
import app.marquee.design.focusSelectable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.delay

/**
 * The TV frame (design/DECISION.md, Navigation): the wordmark at the left and a solid `surface` tab bar
 * centered at the top (68 tall, its top at 46) with every place in words, and the screen under it. The
 * screen starts below the bar rather than behind it: a focus group is a candidate for a D-pad move only
 * when it lies wholly in that direction. Focus moves between them by rule: Up out of the top of a screen
 * enters the bar on the current place, Down from the bar returns to what had focus in the screen
 * ([contentEntry]), and the ends of the bar stop Left and Right. A tab that keeps focus for
 * [TAB_SETTLE_MS] shows its place, as Apple TV's tab bar does; focus stays on it. A held arrow key is
 * throttled here, once, for every screen.
 *
 * @param contentEntry where focus lands when it enters the content from the pill (null: the
 * default search).
 * @param onNavFocus called when focus reaches the pill.
 * @param onKey called when an arrow key or select goes down.
 * @param nav whether the bar holds focus, and a way to put it there (Back from a place's content).
 */
@Composable
fun TvShell(
    selected: Route,
    showNav: Boolean,
    onSelect: (Route) -> Unit,
    contentEntry: () -> FocusRequester?,
    onNavFocus: () -> Unit,
    onKey: () -> Unit,
    modifier: Modifier = Modifier,
    nav: TvNavFocus = remember { TvNavFocus() },
    /**
     * The active profile's avatar at the bar's end, opening its menu (plans/nav-profile.md). `lit` while Settings shows,
     * which has no tab of its own: then `modifier` carries the bar's entry, so Up and Back land on it.
     */
    profileButton: (@Composable (lit: Boolean, modifier: Modifier) -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    // A bar that left (a pushed page) holds nothing; one that comes back reports its focus again.
    if (!showNav) nav.held = false
    val throttle = remember { DpadThrottle() }
    Column(
        modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                if (event.key in dpadKeys) {
                    KeyPresses.pressed()
                    nav.keys++
                    onKey()
                }
                val axis =
                    when (event.key) {
                        Key.DirectionLeft, Key.DirectionRight -> Axis.Horizontal
                        Key.DirectionUp, Key.DirectionDown -> Axis.Vertical
                        else -> return@onPreviewKeyEvent false
                    }
                val native = event.nativeKeyEvent
                !throttle.allow(axis, native.repeatCount > 0, native.eventTime)
            },
    ) {
        if (showNav) {
            // Drawn over the screen: Home's top shelf runs its art up behind the bar (TvNavRoom).
            TvTopNav(
                selected,
                onSelect,
                onNavFocus,
                nav,
                Modifier.zIndex(1f).fillMaxWidth().padding(top = NAV_TOP),
                profileButton,
            )
        }
        Box(
            Modifier
                .fillMaxWidth()
                .weight(1f)
                .focusProperties { enter = { contentEntry() ?: FocusRequester.Default } }
                .focusGroup(),
        ) {
            CompositionLocalProvider(
                TvNavRoom provides if (showNav) NAV_TOP + NAV_HEIGHT else 0.dp,
                LocalTvNav provides nav,
            ) { content() }
        }
    }
}

/**
 * The bar's focus, shared by the shell, the Back chain and Home's first focus. Plain fields, not Compose
 * state: they are read when a key or an entry asks, so a focus move recomposes nothing.
 */
class TvNavFocus {
    internal val current = FocusRequester()

    /** True while a tab of the bar, or its profile button, has focus. */
    var held = false
        internal set

    /**
     * D-pad keys pressed while this bar was composed. It starts again with the activity (a configuration change), so
     * a start that finds the system's default focus on the bar is told apart from a viewer who put it there.
     */
    var keys = 0
        internal set

    /** The place the bar itself asked for, until it shows. */
    internal var asked: Route? = null

    /** Puts focus on the current place's tab; false when the bar is not there. */
    fun take(): Boolean = runCatching { current.requestFocus() }.getOrDefault(false)

    /** Back in a place's content: focus goes to its tab. False when the bar holds focus already (Back goes on) or is gone. */
    fun backToBar(): Boolean = !held && take()

    /** True while the bar holds focus that a key put there (not the system's default focus at a start). */
    val heldByKey: Boolean get() = held && keys > 0
}

/** The bar of the shell around this screen (null outside one). */
val LocalTvNav = compositionLocalOf<TvNavFocus?> { null }

/**
 * How long focus rests on a tab before its place shows: the androidx.tv TabRow's own debounce sample. Longer
 * than a held arrow's pace (DpadThrottle lets a repeat through every 80 ms), so a sweep across the bar opens
 * no place on the way.
 */
internal const val TAB_SETTLE_MS = 250L

/**
 * How far the bar reaches down over the top of the screen: a full-bleed backdrop (Home's top shelf) runs up this
 * far behind it, to the top of the TV. 0 with no bar.
 */
val TvNavRoom = compositionLocalOf { 0.dp }

/** The gap between the bar and a top-level screen's first item. */
@Composable
fun tvContentTop(): Dp = Marquee.space.m

private val dpadKeys =
    setOf(Key.DirectionLeft, Key.DirectionRight, Key.DirectionUp, Key.DirectionDown, Key.DirectionCenter, Key.Enter)

private class Place(
    val label: String,
    val icon: ImageVector,
)

private val places =
    mapOf(
        Route.Search to Place("Search", Icons.Search),
        Route.Home to Place("Home", Icons.House),
        Route.Discover to Place("Discover", Icons.Compass),
        Route.Library to Place("Library", Icons.LibraryBig),
        Route.Calendar to Place("Calendar", Icons.CalendarDays),
    )

/** Browsing places first, then the tools, as on the web; the tools show their icon beside the word. */
private val tools = setOf(Route.Search)

@Composable
private fun TvTopNav(
    selected: Route,
    onSelect: (Route) -> Unit,
    onNavFocus: () -> Unit,
    nav: TvNavFocus,
    modifier: Modifier,
    profileButton: (@Composable (lit: Boolean, modifier: Modifier) -> Unit)?,
) {
    val current = nav.current
    val pill = RoundedCornerShape(Marquee.radius.pill)
    val focusManager = LocalFocusManager.current
    // The tab focus rests on (null: none of them).
    var resting by remember { mutableStateOf<Route?>(null) }
    val shown by rememberUpdatedState(selected)
    val latestSelect by rememberUpdatedState(onSelect)
    val show: (Route) -> Unit = { route ->
        nav.asked = route
        latestSelect(route)
    }
    LaunchedEffect(resting) {
        val tab = resting ?: return@LaunchedEffect
        if (tab == shown) return@LaunchedEffect
        delay(TAB_SETTLE_MS)
        // A Select during the settle showed it already.
        if (tab != shown) show(tab)
    }
    // The place changed from elsewhere while focus sat on another tab (Back from the bar went Home): focus
    // follows it, so the settle never takes the viewer back.
    LaunchedEffect(selected) {
        // The bar's own switch: the viewer may have moved on to another tab in the frame before; that move stands.
        if (selected == nav.asked) {
            nav.asked = null
            return@LaunchedEffect
        }
        val tab = resting ?: return@LaunchedEffect
        if (tab != selected) nav.take()
    }
    // Select on the place shown enters it, as on Apple TV; on another place it shows that one at once.
    val choose: (Route) -> Unit = { route ->
        if (route == selected) focusManager.moveFocus(FocusDirection.Down) else show(route)
    }
    Box(modifier) {
        // The mark: the bird alone where the bar is tight (C); not focusable, not spoken.
        BrandMark(NAV_HEIGHT, Modifier.align(Alignment.CenterStart).padding(start = Marquee.safe.side))
        Row(
            Modifier
                .align(Alignment.Center)
                .height(NAV_HEIGHT)
                .clip(pill)
                .background(Marquee.colors.surface)
                .focusProperties { enter = { current } }
                .onFocusChanged { nav.held = it.hasFocus }
                .focusGroup()
                .padding(horizontal = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Settings has no tab: the profile's menu at the end opens it.
            val tabs = TOP_LEVEL - Route.Settings
            for (route in tabs.filter { it !in tools } + tabs.filter { it in tools }) {
                NavItem(route, route == selected, current, choose, onNavFocus) { gained ->
                    if (gained) {
                        resting = route
                    } else if (resting == route) {
                        resting = null
                    }
                }
            }
            val lit = selected == Route.Settings
            profileButton?.invoke(
                lit,
                Modifier
                    .then(if (lit) Modifier.focusRequester(current) else Modifier)
                    .onFocusChanged { if (it.isFocused) onNavFocus() },
            )
        }
    }
}

@Composable
private fun NavItem(
    route: Route,
    isSelected: Boolean,
    current: FocusRequester,
    onSelect: (Route) -> Unit,
    onNavFocus: () -> Unit,
    onFocus: (Boolean) -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    // A focused tab turns white with dark words, as tvOS draws it (focusPalette).
    val colors = focusPalette(focused)
    val place = places.getValue(route)
    val shape = RoundedCornerShape(Marquee.radius.pill)
    Row(
        Modifier
            .then(if (isSelected) Modifier.focusRequester(current) else Modifier)
            .marqueeFocus(NAV_ITEM / 2, Tokens.FOCUS_SCALE_TILE, rise = false)
            .onFocusChanged {
                focused = it.isFocused
                onFocus(it.isFocused)
                if (it.isFocused) onNavFocus()
            }.height(NAV_ITEM)
            .clip(shape)
            .background(
                when {
                    focused -> colors.surface
                    isSelected -> ACTIVE
                    else -> Color.Transparent
                },
            ).focusSelectable(isSelected, { onSelect(route) }, Role.Tab)
            .padding(horizontal = 26.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Search carries its icon; the browsing places are words alone, as on the web. The
        // active tab is pure white in its pill, the rest 64 percent.
        val ink =
            when {
                focused -> colors.text
                isSelected -> Color.White
                else -> colors.dim
            }
        if (route in tools) MqIcon(place.icon, tint = ink, size = 30.dp)
        MqText(place.label, color = ink, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** The active tab's pill: white at 20 percent. */
private val ACTIVE = Color(0x33FFFFFF)
private val NAV_TOP = 46.dp
private val NAV_HEIGHT = 68.dp
private val NAV_ITEM = 60.dp
