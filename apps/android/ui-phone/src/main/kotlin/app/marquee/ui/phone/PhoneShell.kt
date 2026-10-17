package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
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

/**
 * The phone frame: the screen above a bottom bar with the five top-level places, each an icon with
 * its label. The bar sits above the system navigation bar and is hidden on pages that take the
 * whole screen (a title page, the player).
 */
@Composable
fun PhoneShell(
    selected: Route,
    showTabs: Boolean,
    onSelect: (Route) -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Column(modifier.fillMaxSize()) {
        Box(Modifier.weight(1f).fillMaxWidth()) { content() }
        if (showTabs) PhoneTabs(selected, onSelect)
    }
}

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
        Route.Settings to Place("Settings", Icons.Settings),
    )

@Composable
private fun PhoneTabs(
    selected: Route,
    onSelect: (Route) -> Unit,
) {
    val colors = Marquee.colors
    Column(Modifier.fillMaxWidth().background(colors.bg).navigationBarsPadding()) {
        Box(Modifier.fillMaxWidth().height(1.dp).background(colors.line))
        Row(Modifier.fillMaxWidth().height(TAB_HEIGHT)) {
            for (route in TOP_LEVEL) {
                val place = places.getValue(route)
                val isSelected = route == selected
                Column(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .focusSelectable(isSelected, { onSelect(route) }, Role.Tab),
                    verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    MqIcon(place.icon, tint = if (isSelected) colors.text else colors.muted)
                    MqText(
                        place.label,
                        role = TextRole.Caption,
                        color = if (isSelected) colors.text else colors.muted,
                        weight = if (isSelected) FontWeight.SemiBold else FontWeight.Medium,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

private val TAB_HEIGHT = 64.dp
