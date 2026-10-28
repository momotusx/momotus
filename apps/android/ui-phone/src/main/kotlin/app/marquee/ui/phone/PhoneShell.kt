@file:OptIn(ExperimentalLayoutApi::class)

package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.dropShadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.marquee.core.route.Route
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.focusSelectable
import app.marquee.design.marqueeFocus
import app.marquee.design.windowDp

/**
 * How much of the bottom the floating bar covers (its height, margin and the system navigation
 * bar), so a screen pads its list to scroll clear of it. Zero where there is no bar.
 */
val LocalBarSpace = compositionLocalOf { 0.dp }

/**
 * How much of the top the shell covers where it draws a back link (the status bar and the link's row), so a page
 * keeps clear of both. Zero where it draws none; read it through [pageTop].
 */
private val LocalTopSpace = compositionLocalOf { 0.dp }

/** Where a page's first line may start: under the status bar, or under the back link's row when there is one. */
@Composable
fun pageTop(): Dp = maxOf(WindowInsets.statusBars.asPaddingValues().calculateTopPadding(), LocalTopSpace.current)

/**
 * The phone frame (design/DECISION.md, Navigation): the screen fills the window and the iPhone's
 * floating capsule with five places sits above the system navigation bar, 21 from the sides. The
 * bar shows on top-level places only and hides while the keyboard is up.
 */
@Composable
fun PhoneShell(
    selected: Route,
    showTabs: Boolean,
    onSelect: (Route) -> Unit,
    modifier: Modifier = Modifier,
    /** False for a page whose artwork runs under the status bar and handles the bar itself. */
    statusStrip: Boolean = true,
    /** False for the player, whose picture runs edge to edge and keeps its own cutout padding. */
    insetSides: Boolean = true,
    /** The blue back link of a pushed page ([app.marquee.core.route.hasBackLink]); null draws none. */
    onBack: (() -> Unit)? = null,
    /** The capsule's last tab, the active profile; null leaves its slot empty (no profile known yet). */
    profile: PhoneProfileTab? = null,
    content: @Composable () -> Unit,
) {
    val showBar = showTabs && !WindowInsets.isImeVisible
    // Under 360 dp the bar gives up some margin, so each place keeps more than 48 dp (320 dp: 58).
    val narrow = windowDp().width < 360.dp
    val inset = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
    val space: Dp = if (showBar) BAR_HEIGHT + BAR_MARGIN * 2 + inset else 0.dp
    val linkRow = onBack != null && statusStrip
    val top: Dp = if (linkRow) WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + BACK_HEIGHT else 0.dp
    Box(modifier.fillMaxSize()) {
        // In landscape the navigation bar and a camera cutout can sit at a side: pages keep clear of both.
        val sides = WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal)
        Box(if (insetSides) Modifier.windowInsetsPadding(sides) else Modifier) {
            // One call site for the page, so moving onto or off a back-link page keeps its state and its fade.
            CompositionLocalProvider(LocalBarSpace provides space, LocalTopSpace provides top) { content() }
        }
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
        if (onBack != null) {
            if (statusStrip) {
                // The link's row sits under the strip and over the page, which keeps clear of it through [pageTop].
                Box(
                    Modifier
                        .fillMaxWidth()
                        .windowInsetsPadding(WindowInsets.statusBars)
                        .background(Marquee.colors.bg)
                        .windowInsetsPadding(sides),
                ) { BackLink(onBack) }
            } else {
                // A page whose artwork runs under the status bar gets the link over it, on a dark pill that keeps
                // the blue legible.
                BackLink(
                    onBack,
                    Modifier.windowInsetsPadding(WindowInsets.statusBars).windowInsetsPadding(sides),
                    floating = true,
                )
            }
        }
        if (showBar) {
            PhoneBar(
                selected,
                onSelect,
                profile,
                Modifier
                    .align(Alignment.BottomCenter)
                    .navigationBarsPadding()
                    .padding(
                        start = if (narrow) 8.dp else BAR_SIDE,
                        end = if (narrow) 8.dp else BAR_SIDE,
                        bottom = BAR_MARGIN,
                    ),
            )
        }
    }
}

/** The pushed page's way back, in the key blue as Apple's navigation bar draws it. */
@Composable
private fun BackLink(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    floating: Boolean = false,
) {
    val side = Marquee.safe.side
    val shape = RoundedCornerShape(Marquee.radius.pill)
    Row(
        modifier
            .padding(start = if (floating) side - 4.dp else 0.dp, top = if (floating) 4.dp else 0.dp)
            .height(BACK_HEIGHT)
            .clip(shape)
            .then(if (floating) Modifier.background(Marquee.colors.bg.copy(alpha = BACK_PILL_ALPHA)) else Modifier)
            .focusClickable(onBack, Role.Button)
            .padding(start = if (floating) 8.dp else side - 4.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.ChevronLeft, tint = Marquee.colors.key)
        MqText("Back", role = TextRole.Body, color = Marquee.colors.key)
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
    )

/**
 * Browsing places first, then Search and the profile, as on the web and Apple: five, as the iPhone's. Calendar is a
 * row at the top of Library (D6), which is lit while it shows. Settings has no tab: the profile's menu opens it.
 */
private val order = listOf(Route.Home, Route.Discover, Route.Library, Route.Search)

/** The profile tab's word; the profile's name is in its spoken label. */
private const val PROFILE_LABEL = "Profile"

/**
 * The capsule's last tab (plans/nav-profile.md): the active profile's avatar over "Profile", lit while Settings shows.
 * A button, never a selected tab, so it still opens its menu when lit.
 */
class PhoneProfileTab(
    /** The spoken label: "Profile: <name>". */
    val spoken: String,
    val onClick: () -> Unit,
    val avatar: @Composable () -> Unit,
)

@Composable
private fun PhoneBar(
    selected: Route,
    onSelect: (Route) -> Unit,
    profile: PhoneProfileTab?,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val lit = if (selected == Route.Calendar) Route.Library else selected
    val caption = Marquee.type.caption
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val shape = RoundedCornerShape(Marquee.radius.pill)
    // The bar's own width is the room, so the labels are fitted before the first frame draws: the widest word at the
    // largest size (the caption, then 8 percent smaller a step, down to a floor) that fits its slot, else the icons
    // alone, each place keeping its label as its spoken name.
    BoxWithConstraints(modifier) {
        val room = with(density) { ((maxWidth - 8.dp) / (order.size + 1) - LABEL_PAD * 2).toPx() }
        val labelSize =
            remember(measurer, caption, density.density, density.fontScale, room) {
                fitLabels(measurer, caption, room)
            }
        Row(
            Modifier
                .fillMaxWidth()
                .height(BAR_HEIGHT)
                .dropShadow(shape) {
                    radius = 30.dp.toPx()
                    offset = Offset(0f, 10.dp.toPx())
                    color = Color.Black
                    alpha = 0.45f
                }.clip(shape)
                // Opaque: Android blurs no content behind a node, so the capsule's translucent fill is laid on the
                // page color, or the text under the bar would show through it (seen on the emulator).
                .background(colors.glassStrong.compositeOver(colors.bg))
                // The frame's inner top highlight, so the capsule's edge reads on the black page.
                .drawBehind {
                    val line = 1.dp.toPx()
                    drawLine(colors.lineStrong, Offset(0f, line / 2), Offset(size.width, line / 2), line)
                }.padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            for (route in order) {
                val place = places.getValue(route)
                val isSelected = route == lit
                Slot(
                    place.label,
                    isSelected,
                    labelSize,
                    Modifier
                        .semantics { contentDescription = place.label }
                        .focusSelectable(isSelected, { onSelect(route) }, Role.Tab),
                ) { ink -> MqIcon(place.icon, tint = ink) }
            }
            // Lit while Settings shows, which has no tab; still a button that opens the menu.
            if (profile != null) {
                Slot(
                    PROFILE_LABEL,
                    lit == Route.Settings,
                    labelSize,
                    Modifier
                        .semantics { contentDescription = profile.spoken }
                        .focusClickable(profile.onClick, Role.Button),
                ) { profile.avatar() }
            } else {
                Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** One of the capsule's five slots: its icon over its fitted word, the lit one in the key blue on a darker inner capsule. */
@Composable
private fun RowScope.Slot(
    label: String,
    isSelected: Boolean,
    labelSize: TextUnit?,
    action: Modifier,
    icon: @Composable (ink: Color) -> Unit,
) {
    val shape = RoundedCornerShape(Marquee.radius.pill)
    // The selected place in the key blue on a darker inner capsule; the rest white.
    val ink = if (isSelected) Marquee.colors.key else Color.White
    Column(
        Modifier
            .weight(1f)
            .height(TAB_HEIGHT)
            // A keyboard on a phone or a tablet sees where it is, apart from the selected place.
            .marqueeFocus(TAB_HEIGHT / 2, scale = 1f)
            .clip(shape)
            .background(if (isSelected) TAB_ON else Color.Transparent)
            .then(action),
        verticalArrangement = Arrangement.spacedBy(1.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        icon(ink)
        if (labelSize != null) {
            BasicText(
                label,
                modifier =
                    Modifier
                        .padding(horizontal = LABEL_PAD)
                        .clearAndSetSemantics {
                            testTag = TAB_LABEL_TAG
                            this[LabelSizeKey] = labelSize.value
                        },
                style =
                    TextStyle(
                        color = ink,
                        fontSize = labelSize,
                        fontFamily = MarqueeFonts.text,
                        fontWeight = FontWeight.SemiBold,
                        lineHeight = labelSize * 1.4f,
                    ),
                maxLines = 1,
                softWrap = false,
                overflow = TextOverflow.Clip,
            )
        }
    }
}

/** The largest label size, from [caption] down to the floor, at which the widest label fits [room] px; null if none. */
private fun fitLabels(
    measurer: TextMeasurer,
    caption: TextUnit,
    room: Float,
): TextUnit? {
    var size = caption
    while (true) {
        val style = TextStyle(fontSize = size, fontFamily = MarqueeFonts.text, fontWeight = FontWeight.SemiBold)
        val widest =
            (order.map { places.getValue(it).label } + PROFILE_LABEL).maxOf {
                measurer.measure(it, style, softWrap = false, maxLines = 1).size.width
            }
        if (widest <= room) return size
        if (size.value <= LABEL_FLOOR_SP) return null
        size = maxOf(size.value * LABEL_STEP, LABEL_FLOOR_SP).sp
    }
}

/** The selected place's inner capsule. */
private val TAB_ON = Color(0xEB0A0A0A)
private val BAR_HEIGHT = 62.dp
private val TAB_HEIGHT = 54.dp
private val BAR_SIDE = 21.dp
private val BAR_MARGIN = 12.dp

/** The tab labels' floor (sp, so it still follows the user's text size), the step down to it, and their side room. */
private const val LABEL_FLOOR_SP = 10f
private const val LABEL_STEP = 0.92f
private val LABEL_PAD = 2.dp

/** The semantics tag of a tab's label, and the size (sp) it was drawn at, which the tests read. */
internal const val TAB_LABEL_TAG = "tab-label"
internal val LabelSizeKey = SemanticsPropertyKey<Float>("LabelSize")

/** A back link's height: Material's 48 dp target. */
internal val BACK_HEIGHT = 48.dp

/** The floating link's pill over title art: at 0.9 the key blue keeps 4.5:1 even over pure white art. */
internal const val BACK_PILL_ALPHA = 0.9f
