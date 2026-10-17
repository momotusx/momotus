@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.marquee.core.route.Route
import app.marquee.core.route.TOP_LEVEL
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.TextRole
import app.marquee.design.focusSelectable
import app.marquee.design.marqueeFocus

/**
 * The TV frame: a permanent labeled rail on the left for the five top-level places, and the
 * screen on the right. Focus moves between them by rule, not by geometry: Left out of a row's
 * first tile enters the rail on the current place, and Right from the rail returns to the tile
 * that had focus. A held arrow key is throttled here, once, for every screen.
 *
 * @param contentEntry where focus lands when it enters the content from the rail (null: the
 * default search).
 * @param onRailFocus called when focus reaches the rail.
 * @param onKey called when an arrow key or select goes down.
 */
@Composable
fun TvShell(
    selected: Route,
    showRail: Boolean,
    onSelect: (Route) -> Unit,
    contentEntry: () -> FocusRequester?,
    onRailFocus: () -> Unit,
    onKey: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val throttle = remember { DpadThrottle() }
    Row(
        modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                if (event.key in dpadKeys) onKey()
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
        if (showRail) TvRail(selected, onSelect, onRailFocus)
        Box(
            Modifier
                .weight(1f)
                .fillMaxHeight()
                .focusProperties { enter = { contentEntry() ?: FocusRequester.Default } }
                .focusGroup(),
        ) { content() }
    }
}

private val dpadKeys =
    setOf(Key.DirectionLeft, Key.DirectionRight, Key.DirectionUp, Key.DirectionDown, Key.DirectionCenter, Key.Enter)

private class Place(
    val route: Route,
    val label: String,
    val icon: ImageVector,
)

private val places =
    mapOf(
        Route.Search to Place(Route.Search, "Search", Icons.Search),
        Route.Home to Place(Route.Home, "Home", Icons.House),
        Route.Discover to Place(Route.Discover, "Discover", Icons.Compass),
        Route.Library to Place(Route.Library, "Library", Icons.LibraryBig),
        Route.Settings to Place(Route.Settings, "Settings", Icons.Settings),
    )

@Composable
private fun TvRail(
    selected: Route,
    onSelect: (Route) -> Unit,
    onRailFocus: () -> Unit,
) {
    val safe = Marquee.safe
    val colors = Marquee.colors
    val current = remember { FocusRequester() }
    Column(
        Modifier
            .width(RAIL_WIDTH)
            .fillMaxHeight()
            .focusProperties { enter = { current } }
            .focusGroup()
            .padding(start = safe.side / 2, end = 40.dp, top = safe.top, bottom = safe.bottom),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        MqText("Marquee", role = TextRole.Title2, modifier = Modifier.padding(start = 28.dp, bottom = 24.dp))
        for (route in TOP_LEVEL) {
            val place = places.getValue(route)
            val isSelected = route == selected
            Row(
                Modifier
                    .fillMaxWidth()
                    .then(if (isSelected) Modifier.focusRequester(current) else Modifier)
                    .marqueeFocus(RAIL_ITEM / 2)
                    .onFocusChanged { if (it.isFocused) onRailFocus() }
                    .height(RAIL_ITEM)
                    .clip(RoundedCornerShape(RAIL_ITEM / 2))
                    .background(if (isSelected) colors.raised else colors.bg)
                    .focusSelectable(isSelected, { onSelect(route) }, Role.Tab)
                    .padding(horizontal = 28.dp),
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MqIcon(place.icon, tint = if (isSelected) colors.text else colors.muted)
                MqText(
                    place.label,
                    color = if (isSelected) colors.text else colors.muted,
                    weight = if (isSelected) FontWeight.SemiBold else FontWeight.Medium,
                    maxLines = 1,
                )
            }
        }
    }
}

private val RAIL_WIDTH = 340.dp
private val RAIL_ITEM = 72.dp
