@file:OptIn(ExperimentalLayoutApi::class)

package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
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
 * How much of the bottom the floating bar covers (its height, margin and the system navigation
 * bar), so a screen pads its list to scroll clear of it. Zero where there is no bar.
 */
val LocalBarSpace = compositionLocalOf { 0.dp }

/**
 * The phone frame (plans/android-design-v2.md section 3): the screen fills the window and a
 * floating glass bar with the top-level places sits above the system navigation bar. The bar
 * shows on top-level places only and hides while the keyboard is up.
 */
@Composable
fun PhoneShell(
    selected: Route,
    showTabs: Boolean,
    onSelect: (Route) -> Unit,
    modifier: Modifier = Modifier,
    /** False for a page whose artwork runs under the status bar and handles the bar itself. */
    statusStrip: Boolean = true,
    content: @Composable () -> Unit,
) {
    val showBar = showTabs && !WindowInsets.isImeVisible
    // Under 360 dp the bar gives up some margin, so six places keep 48 dp each (320 dp: 50).
    val narrow = LocalConfiguration.current.screenWidthDp < 360
    val inset = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
    val space: Dp = if (showBar) BAR_HEIGHT + BAR_MARGIN * 2 + inset else 0.dp
    Box(modifier.fillMaxSize()) {
        CompositionLocalProvider(LocalBarSpace provides space) { content() }
        // Pages start below the status bar but scroll under it: a strip of page color keeps the
        // text off the clock and icons. At rest it covers only the page's own background.
        if (statusStrip) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .windowInsetsTopHeight(WindowInsets.statusBars)
                    .background(Marquee.colors.bg),
            )
        }
        if (showBar) {
            PhoneBar(
                selected,
                onSelect,
                Modifier
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(
                        start = if (narrow) 8.dp else 16.dp,
                        end = if (narrow) 8.dp else 16.dp,
                        bottom = BAR_MARGIN,
                    ),
            )
        }
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
        Route.Calendar to Place("Calendar", Icons.CalendarDays),
        Route.Settings to Place("Settings", Icons.Settings),
    )

/** Browsing places first, then the tools, as on the web and Apple. */
private val order =
    listOf(Route.Home, Route.Discover, Route.Library, Route.Calendar, Route.Search, Route.Settings)

@Composable
private fun PhoneBar(
    selected: Route,
    onSelect: (Route) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    // Narrow screens and large text keep the icons; each keeps its label as its spoken name.
    val compact = LocalConfiguration.current.screenWidthDp < 360 || LocalDensity.current.fontScale > 1.3f
    val shape = RoundedCornerShape(Marquee.radius.l)
    Row(
        modifier
            .fillMaxWidth()
            .height(BAR_HEIGHT)
            .clip(shape)
            // Opaque: Android blurs no content behind a node, and the translucent glass let the
            // text under the bar show through it (seen on the emulator).
            .background(colors.glassStrong.compositeOver(colors.bg))
            .border(1.dp, colors.control, shape)
            .padding(if (compact) 2.dp else 6.dp),
    ) {
        for (route in order.filter { it in TOP_LEVEL }) {
            val place = places.getValue(route)
            val isSelected = route == selected
            Column(
                Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .clip(RoundedCornerShape(Marquee.radius.m))
                    .background(if (isSelected) colors.text.copy(alpha = 0.12f) else Color.Transparent)
                    .semantics { contentDescription = place.label }
                    .focusSelectable(isSelected, { onSelect(route) }, Role.Tab),
                verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                MqIcon(place.icon, tint = if (isSelected) colors.text else colors.muted)
                if (!compact) {
                    MqText(
                        place.label,
                        modifier = Modifier.clearAndSetSemantics { },
                        role = TextRole.Caption,
                        color = if (isSelected) colors.text else colors.muted,
                        weight = if (isSelected) FontWeight.Bold else FontWeight.SemiBold,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

private val BAR_HEIGHT = 64.dp
private val BAR_MARGIN = 12.dp
