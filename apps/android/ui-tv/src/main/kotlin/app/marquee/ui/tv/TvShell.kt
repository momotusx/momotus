@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.route.Route
import app.marquee.core.route.TOP_LEVEL
import app.marquee.design.Icons
import app.marquee.design.KeyPresses
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.TextRole
import app.marquee.design.focusSelectable
import app.marquee.design.marqueeFocus

/**
 * The TV frame (plans/android-design-v2.md section 2): a glass pill centered at the top with the
 * top-level places, and the screen under it. The screen starts below the pill rather than behind
 * it: a focus group is a candidate for a D-pad move only when it lies wholly in that direction. Focus moves between them by rule:
 * Up out of the top of a screen enters the pill on the current place, Down from the pill returns
 * to what had focus in the screen ([contentEntry]), and the ends of the pill stop Left and Right.
 * A held arrow key is throttled here, once, for every screen.
 *
 * @param contentEntry where focus lands when it enters the content from the pill (null: the
 * default search).
 * @param onNavFocus called when focus reaches the pill.
 * @param onKey called when an arrow key or select goes down.
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
    /** The active profile's avatar at the pill's end; it opens the picker. */
    profileButton: (@Composable () -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val throttle = remember { DpadThrottle() }
    Column(
        modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                if (event.key in dpadKeys) {
                    KeyPresses.pressed()
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
            TvTopNav(
                selected,
                onSelect,
                onNavFocus,
                Modifier.align(Alignment.CenterHorizontally).padding(top = Marquee.safe.top),
                profileButton,
            )
        }
        Box(
            Modifier
                .fillMaxWidth()
                .weight(1f)
                .focusProperties { enter = { contentEntry() ?: FocusRequester.Default } }
                .focusGroup(),
        ) { content() }
    }
}

/** The gap between the pill and a top-level screen's first item. */
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
        Route.Settings to Place("Settings", Icons.Settings),
    )

/** Browsing places first, then the tools after a divider, as on the web. */
private val tools = setOf(Route.Search, Route.Settings)

@Composable
private fun TvTopNav(
    selected: Route,
    onSelect: (Route) -> Unit,
    onNavFocus: () -> Unit,
    modifier: Modifier,
    profileButton: (@Composable () -> Unit)?,
) {
    val colors = Marquee.colors
    val current = remember { FocusRequester() }
    val pill = RoundedCornerShape(Marquee.radius.pill)
    val browse = TOP_LEVEL.filter { it !in tools }
    val tail = TOP_LEVEL.filter { it in tools }
    Row(
        modifier
            .height(NAV_HEIGHT)
            .clip(pill)
            .background(colors.glassStrong)
            .border(1.dp, colors.edge, pill)
            .focusProperties { enter = { current } }
            .focusGroup()
            .padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // The wordmark: an accent dot and the name; not focusable.
        Row(
            Modifier.padding(start = 16.dp, end = 20.dp).clearAndSetSemantics { },
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(10.dp).clip(CircleShape).background(colors.accent))
            MqText("Marquee", role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
        }
        for (route in browse) NavItem(route, route == selected, current, onSelect, onNavFocus)
        Box(
            Modifier
                .padding(horizontal = 12.dp)
                .width(1.dp)
                .height(32.dp)
                .background(colors.lineStrong),
        )
        for (route in tail) NavItem(route, route == selected, current, onSelect, onNavFocus)
        profileButton?.invoke()
    }
}

@Composable
private fun NavItem(
    route: Route,
    isSelected: Boolean,
    current: FocusRequester,
    onSelect: (Route) -> Unit,
    onNavFocus: () -> Unit,
) {
    val colors = Marquee.colors
    val place = places.getValue(route)
    val shape = RoundedCornerShape(Marquee.radius.pill)
    Row(
        Modifier
            .then(if (isSelected) Modifier.focusRequester(current) else Modifier)
            .marqueeFocus(NAV_ITEM / 2)
            .onFocusChanged { if (it.isFocused) onNavFocus() }
            .height(NAV_ITEM)
            .clip(shape)
            .background(if (isSelected) colors.text.copy(alpha = 0.12f) else Color.Transparent)
            .focusSelectable(isSelected, { onSelect(route) }, Role.Tab)
            .padding(horizontal = 22.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Search and Settings carry their icon; the browsing places are words alone, as on the web.
        if (route in tools) MqIcon(place.icon, tint = if (isSelected) colors.text else colors.muted, size = 26.dp)
        MqText(
            place.label,
            role = TextRole.Callout,
            color = if (isSelected) colors.text else colors.muted,
            weight = if (isSelected) FontWeight.Bold else FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

private val NAV_HEIGHT = 72.dp
private val NAV_ITEM = 56.dp
