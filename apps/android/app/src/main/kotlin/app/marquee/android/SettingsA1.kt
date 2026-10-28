@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.android

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.BiasAlignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import app.marquee.core.AccountCopy
import app.marquee.core.TileCopy
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.Tile
import app.marquee.design.Backdrop
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.ScrimStop
import app.marquee.design.Scrims
import app.marquee.design.SecondaryButton
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus
import app.marquee.ui.phone.LocalBarSpace
import app.marquee.ui.phone.pageTop
import app.marquee.ui.tv.TvNavRoom
import coil3.compose.AsyncImage
import kotlinx.coroutines.launch

// Settings A1 (the owner's pick of 2026-10-28, design/DECISION.md): the profile on a film's backdrop, then one long
// page of grouped lists with blue-to-green icon tiles; TV adds a section menu at the left of one centered column.

/** An icon tile's fill: one of the blue-to-green steps, an owner-picked exception to the one key color. */
internal enum class Tone { Blue, Sky, Sea, Teal, Mint, Green, Grey }

@Composable
@ReadOnlyComposable
internal fun Tone.fill(): Color =
    Marquee.colors.let {
        when (this) {
            Tone.Blue -> it.tileBlue
            Tone.Sky -> it.tileSky
            Tone.Sea -> it.tileSea
            Tone.Teal -> it.tileTeal
            Tone.Mint -> it.tileMint
            Tone.Green -> it.tileGreen
            Tone.Grey -> it.tileGrey
        }
    }

/** The page's sections, in page order. Profiles is the header. */
internal enum class SettingsSection(
    val label: String,
    val tone: Tone,
    val icon: ImageVector,
) {
    Profiles("Profiles", Tone.Blue, Icons.User),
    Sources("Sources", Tone.Green, Icons.Puzzle),
    Account("Account", Tone.Sky, Icons.MonitorSmartphone),
    Home("Home", Tone.Mint, Icons.Rows3),
    Playback("Playback", Tone.Teal, Icons.CirclePlay),
    Device("This device", Tone.Sea, Icons.Monitor),
    About("About", Tone.Grey, Icons.Info),
}

/** One section of the list: its groups, under its title. */
internal class SettingsPart(
    val section: SettingsSection,
    val content: @Composable () -> Unit,
)

/**
 * The header's film: the first Continue Watching title with a backdrop, else Home's first hero with one. A kids
 * profile takes its own Continue Watching only.
 */
internal fun settingsArt(
    rows: List<HomeRow>,
    heroes: List<Hero>,
    kids: Boolean,
): Tile? =
    rows.firstOrNull { it.rowId == TileCopy.CONTINUE_ROW }?.tiles?.firstOrNull { it.background != null }
        ?: heroes.takeUnless { kids }?.firstOrNull { it.tile.background != null }?.tile

/** A white glyph on one rounded tile: 28 on the phone, 48 on TV. Decoration: the row's words say what it is. */
@Composable
internal fun IconTile(
    icon: ImageVector,
    tone: Tone,
) {
    val tv = Marquee.form == Form.Tv
    val size = if (tv) 48.dp else 28.dp
    Box(
        Modifier.size(size).clip(RoundedCornerShape(if (tv) 12.dp else 7.dp)).background(tone.fill()),
        contentAlignment = Alignment.Center,
    ) { MqIcon(icon, tint = Color.White, size = size * 0.6f) }
}

/** A section's name over its groups, a heading for screen readers. */
@Composable
internal fun SettingsTitle(text: String) {
    if (Marquee.form == Form.Tv) {
        MqText(
            text,
            Modifier.padding(top = 8.dp, bottom = 20.dp).semantics { heading() },
            role = TextRole.Title3,
            weight = FontWeight.Bold,
        )
    } else {
        MqText(
            text,
            Modifier.padding(start = 16.dp, bottom = 8.dp).semantics { heading() },
            role = TextRole.Callout,
            weight = FontWeight.SemiBold,
            color = Marquee.colors.dim,
        )
    }
}

/** One solid `surface` group of rows (a null slot is a row not shown), hairlines between them from the title on. */
@Composable
internal fun SettingsGroup(vararg slots: (@Composable () -> Unit)?) {
    val rows = slots.filterNotNull()
    val tv = Marquee.form == Form.Tv
    Column(Modifier.fillMaxWidth().background(Marquee.colors.surface, RoundedCornerShape(groupRadius()))) {
        rows.forEachIndexed { i, row ->
            if (i > 0) {
                Box(
                    Modifier
                        .padding(start = if (tv) 106.dp else 58.dp)
                        .fillMaxWidth()
                        .height(if (tv) 1.dp else 0.5.dp)
                        .background(if (tv) Marquee.colors.line else Color.White.copy(alpha = 0.16f)),
                )
            }
            row()
        }
    }
}

@Composable
@ReadOnlyComposable
internal fun groupRadius(): Dp = if (Marquee.form == Form.Tv) Marquee.radius.l else 26.dp

/** A row that opens another page: its tile, its title and line, a chevron at 40 percent. On TV the focused row is white. */
@Composable
internal fun SettingsRow(
    icon: ImageVector,
    tone: Tone,
    title: String,
    text: String,
    onClick: () -> Unit,
    lines: Int = 3,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(groupRadius())
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .fillMaxWidth()
                .marqueeFocus(groupRadius(), Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(if (focused && tv) colors.surface else Color.Transparent)
                .semantics(mergeDescendants = true) {}
                .focusClickable(onClick, Role.Button)
                .heightIn(min = if (tv) 104.dp else 51.dp)
                .padding(horizontal = if (tv) 30.dp else 16.dp, vertical = if (tv) 18.dp else 10.dp),
            horizontalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconTile(icon, tone)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(if (tv) 4.dp else 2.dp)) {
                MqText(title, role = TextRole.Body, weight = if (tv) FontWeight.SemiBold else null, maxLines = 2)
                if (text.isNotEmpty()) {
                    MqText(
                        text,
                        role = if (tv) TextRole.Callout else TextRole.Caption,
                        color = colors.dim,
                        maxLines = lines,
                    )
                }
            }
            // Points the way the page goes: left in a right-to-left layout.
            val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
            MqIcon(
                Icons.ChevronRight,
                Modifier.graphicsLayer { scaleX = if (rtl) -1f else 1f },
                tint = if (focused && tv) colors.dim else colors.text.copy(alpha = 0.4f),
            )
        }
    }
}

/**
 * The header's art: the film's backdrop under its ramps in the page color. Nothing shows until the picture lands, so
 * the header is the same with art, before it and with none; then the art fades in by opacity alone.
 */
@Composable
private fun HeaderArt(
    url: String,
    modifier: Modifier,
    side: List<ScrimStop>,
    foot: List<ScrimStop>,
) {
    var landed by remember(url) { mutableStateOf(false) }
    val alpha by animateFloatAsState(if (landed) 1f else 0f, tween(Marquee.panelMs), label = "art")
    Backdrop(
        url,
        modifier.testTag(SETTINGS_ART),
        side = side,
        foot = foot,
        alignment = BiasAlignment(0f, -0.4f),
        artAlpha = { alpha },
        onSuccess = { landed = true },
        ground = Color.Transparent,
    )
}

/** The film's own logo, decorative; none when the title has none or it does not load. */
@Composable
private fun FilmLogo(
    url: String,
    modifier: Modifier,
) {
    var failed by remember(url) { mutableStateOf(false) }
    if (!failed) {
        AsyncImage(
            model = url,
            contentDescription = null,
            contentScale = ContentScale.Fit,
            alignment = Alignment.BottomEnd,
            onError = { failed = true },
            modifier = modifier,
        )
    }
}

/**
 * The profile Edit profile and Manage profiles act on: none on a kids profile, which leaves only with the owner's PIN
 * (design/DECISION.md, kids rules).
 */
internal fun editableProfile(
    me: ProfileInfo?,
    kids: Boolean,
): ProfileInfo? = me?.takeIf { !it.kids && !kids }

/** What the header says under the name. */
internal fun profileLine(many: Boolean): String =
    if (many) AccountCopy.SWITCH_PROFILE else "Add a profile for someone else, or switch"

/**
 * The phone: the art at the top, the large title and the profile as one glass card on its lower half (one tap to the
 * picker), then the groups with their section names as group titles.
 */
@Composable
internal fun PhoneSettings(
    me: ProfileInfo?,
    line: String,
    art: Tile?,
    onProfiles: () -> Unit,
    parts: List<SettingsPart>,
) {
    LazyColumn(
        Modifier.fillMaxSize().testTag(SETTINGS_LIST),
        contentPadding = PaddingValues(bottom = Marquee.space.l + LocalBarSpace.current),
    ) {
        item(key = "header") {
            Box(Modifier.fillMaxWidth().heightIn(min = PHONE_HEADER).testTag(SETTINGS_HEADER)) {
                art?.background?.let {
                    HeaderArt(
                        it,
                        Modifier.matchParentSize(),
                        emptyList(),
                        Scrims.settingsPhoneFoot,
                    )
                }
                Column(
                    Modifier
                        .align(Alignment.BottomStart)
                        .fillMaxWidth()
                        .padding(start = 16.dp, end = 16.dp, top = pageTop(), bottom = 18.dp),
                ) {
                    MqText(
                        "Settings",
                        Modifier.padding(start = 4.dp, bottom = 18.dp).semantics { heading() },
                        role = TextRole.Hero,
                        weight = FontWeight.Bold,
                        color = Color.White,
                    )
                    if (me != null) ProfileCard(me, line, onProfiles)
                }
            }
        }
        itemsIndexed(parts, key = { _, part -> part.section.name }) { i, part ->
            Column(Modifier.padding(start = 16.dp, end = 16.dp, top = if (i == 0) 18.dp else 30.dp)) {
                SettingsTitle(part.section.label)
                part.content()
            }
        }
    }
}

/** The phone header's profile: one glass card, one tap to the picker. */
@Composable
private fun ProfileCard(
    me: ProfileInfo,
    line: String,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(26.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .marqueeFocus(26.dp)
            .clip(shape)
            .background(PHONE_CARD)
            .semantics(mergeDescendants = true) {}
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ProfileAvatar(me, 56.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(
                me.name,
                Modifier.semantics { heading() },
                role = TextRole.Title3,
                weight = FontWeight.SemiBold,
                maxLines = 2,
            )
            MqText(line, role = TextRole.Caption, color = Marquee.colors.dim)
        }
        MqIcon(Icons.ChevronRight, tint = Marquee.colors.text.copy(alpha = 0.5f))
    }
}

/**
 * TV: the header on the film, then the section menu at the left of one centered column and the grouped lists at its
 * right. The profile takes the first focus; the header scrolls away once focus goes down into the menu (past its
 * Profiles entry) or the lists, and the menu stays in place while the lists scroll. Select on an entry brings its
 * section to the top and takes the remote into it; the section at the top of the lists fills its entry.
 */
@Composable
internal fun TvSettings(
    me: ProfileInfo?,
    line: String,
    art: Tile?,
    canEdit: Boolean,
    canManage: Boolean,
    onProfiles: () -> Unit,
    onEdit: () -> Unit,
    onManage: () -> Unit,
    parts: List<SettingsPart>,
) {
    val profile = remember { FocusRequester() }
    val entries = remember { SettingsSection.entries.associateWith { FocusRequester() } }
    val sections = remember { SettingsSection.entries.associateWith { FocusRequester() } }
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    var menuFocus by remember { mutableStateOf<SettingsSection?>(null) }
    var listFocus by remember { mutableStateOf(false) }
    val away = listFocus || (menuFocus != null && menuFocus != SettingsSection.Profiles)
    val shown by animateFloatAsState(
        if (away) 0f else 1f,
        tween(Marquee.panelMs, easing = Tokens.ease),
        label = "header",
    )
    val atTop by remember(parts) {
        derivedStateOf {
            val info = list.layoutInfo
            // The topmost section still showing under the list's top edge (offsets start under its padding): the one an
            // entry brought to the top.
            info.visibleItemsInfo.firstOrNull { it.offset + it.size > 0 }?.index ?: list.firstVisibleItemIndex
        }
    }
    val active = if (away) parts.getOrNull(atTop)?.section ?: SettingsSection.Sources else SettingsSection.Profiles
    AutoFocus(profile)
    BoxWithConstraints(Modifier.fillMaxSize().focusRestorer(profile)) {
        val pad = maxOf(Marquee.safe.side, (maxWidth - TV_COLUMN) / 2)
        val tail = maxHeight * 0.6f
        Column(Modifier.fillMaxSize()) {
            Box(
                Modifier.fillMaxWidth().layout { measurable, constraints ->
                    val full = TV_HEADER.roundToPx()
                    val h = (full * shown).toInt()
                    val header = measurable.measure(Constraints.fixed(constraints.maxWidth, full))
                    layout(constraints.maxWidth, h) { header.place(0, h - full) }
                },
            ) {
                TvHeader(
                    me,
                    line,
                    art,
                    canEdit,
                    canManage,
                    onProfiles,
                    onEdit,
                    onManage,
                    profile,
                    entries.getValue(SettingsSection.Profiles),
                    pad,
                )
            }
            Row(Modifier.fillMaxWidth().weight(1f).padding(horizontal = pad)) {
                Column(
                    Modifier
                        .width(TV_MENU)
                        .padding(top = 32.dp)
                        .focusProperties { enter = { entries.getValue(active) } }
                        .focusGroup()
                        .testTag(SETTINGS_MENU),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    MqText(
                        "Settings",
                        Modifier.padding(start = 20.dp, end = 20.dp, bottom = 12.dp).semantics { heading() },
                        role = TextRole.Callout,
                        weight = FontWeight.Bold,
                        color = Marquee.colors.dim,
                    )
                    val menu = listOf(SettingsSection.Profiles) + parts.map { it.section }
                    for (s in menu) {
                        MenuEntry(
                            s,
                            active = s == active,
                            requester = entries.getValue(s),
                            up = profile.takeIf { s == SettingsSection.Profiles },
                            onFocus = { on ->
                                if (on) {
                                    menuFocus = s
                                } else if (menuFocus == s) {
                                    menuFocus = null
                                }
                            },
                            onPick = {
                                if (s == SettingsSection.Profiles) {
                                    runCatching { profile.requestFocus() }
                                } else {
                                    val index = parts.indexOfFirst { it.section == s }
                                    scope.launch {
                                        list.animateScrollToItem(index)
                                        runCatching { sections.getValue(s).requestFocus(FocusDirection.Enter) }
                                    }
                                }
                            },
                        )
                    }
                }
                Spacer(Modifier.width(64.dp))
                LazyColumn(
                    Modifier
                        .weight(
                            1f,
                        ).fillMaxHeight()
                        .onFocusChanged { listFocus = it.hasFocus }
                        .testTag(SETTINGS_LIST),
                    state = list,
                    contentPadding = PaddingValues(top = 32.dp, bottom = tail),
                    verticalArrangement = Arrangement.spacedBy(56.dp),
                ) {
                    itemsIndexed(parts, key = { _, part -> part.section.name }) { _, part ->
                        Column(
                            Modifier
                                .focusRequester(sections.getValue(part.section))
                                .focusGroup(),
                        ) {
                            SettingsTitle(part.section.label)
                            part.content()
                        }
                    }
                }
            }
        }
    }
}

/** TV's header: the profile, its name and line (the whole block opens the picker), Edit profile and Manage profiles. */
@Composable
private fun TvHeader(
    me: ProfileInfo?,
    line: String,
    art: Tile?,
    canEdit: Boolean,
    canManage: Boolean,
    onProfiles: () -> Unit,
    onEdit: () -> Unit,
    onManage: () -> Unit,
    profile: FocusRequester,
    down: FocusRequester,
    pad: Dp,
) {
    val room = TvNavRoom.current
    Box(Modifier.fillMaxSize().testTag(SETTINGS_HEADER)) {
        art?.background?.let { url ->
            // The art runs up behind the tab bar to the top of the TV, as Home's top shelf.
            HeaderArt(
                url,
                Modifier.matchParentSize().layout { measurable, constraints ->
                    val up = room.roundToPx()
                    val p = measurable.measure(Constraints.fixed(constraints.maxWidth, constraints.maxHeight + up))
                    layout(constraints.maxWidth, constraints.maxHeight) { p.place(0, -up) }
                },
                Scrims.settingsSide,
                Scrims.settingsFoot,
            )
        }
        Row(
            Modifier.align(Alignment.BottomStart).fillMaxWidth().padding(start = pad, end = pad, bottom = 64.dp),
            horizontalArrangement = Arrangement.spacedBy(40.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            Column(Modifier.weight(1f)) {
                if (me != null) {
                    ProfileBlock(me, line, onProfiles, profile, down)
                    if (canEdit) {
                        Row(
                            Modifier.padding(start = TV_AVATAR + 40.dp, top = 28.dp),
                            horizontalArrangement = Arrangement.spacedBy(20.dp),
                        ) {
                            val toMenu = Modifier.focusProperties { this.down = down }
                            SecondaryButton("Edit profile", onEdit, toMenu, icon = Icons.UserPen)
                            if (canManage) SecondaryButton("Manage profiles", onManage, toMenu, icon = Icons.Users)
                        }
                    }
                }
            }
            art?.logo?.let { FilmLogo(it, Modifier.size(380.dp, 150.dp)) }
        }
    }
}

/** The profile on TV: its focus turns the avatar white and lifts it, as a profile in the picker. */
@Composable
private fun ProfileBlock(
    me: ProfileInfo,
    line: String,
    onClick: () -> Unit,
    requester: FocusRequester,
    down: FocusRequester,
) {
    var focused by remember { mutableStateOf(false) }
    val lift by animateFloatAsState(
        if (focused) 1f else 0f,
        tween(Marquee.focusMs, easing = Tokens.ease),
        label = "lift",
    )
    Row(
        Modifier
            .focusRequester(requester)
            .focusProperties { this.down = down }
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) {}
            .focusClickable(onClick, Role.Button)
            .testTag(SETTINGS_PROFILE),
        horizontalArrangement = Arrangement.spacedBy(40.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.graphicsLayer {
                val scale = 1f + (Tokens.FOCUS_SCALE_ART - 1f) * lift
                scaleX = scale
                scaleY = scale
                translationY = -Tokens.focusRiseArt.toPx() * lift
            },
        ) { ProfileAvatar(me, TV_AVATAR, lit = focused) }
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            MqText(
                me.name,
                Modifier.semantics {
                    heading()
                },
                role = TextRole.Title1,
                weight = FontWeight.Bold,
                color = Color.White,
                maxLines = 2,
            )
            MqText(line, role = TextRole.Body, color = Marquee.colors.muted)
        }
    }
}

/** A section menu entry: its tile and word; the section at the top of the lists fills it grey, a focus fills it white. */
@Composable
private fun MenuEntry(
    section: SettingsSection,
    active: Boolean,
    requester: FocusRequester,
    up: FocusRequester?,
    onFocus: (Boolean) -> Unit,
    onPick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .fillMaxWidth()
                .height(84.dp)
                .focusRequester(requester)
                .then(if (up != null) Modifier.focusProperties { this.up = up } else Modifier)
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged {
                    focused = it.isFocused
                    onFocus(it.isFocused)
                }.clip(shape)
                .background(
                    when {
                        focused -> colors.surface
                        active -> Tokens.selectedFill
                        else -> Color.Transparent
                    },
                ).semantics { selected = active }
                .focusClickable(onPick, Role.Tab)
                .padding(horizontal = 20.dp),
            horizontalArrangement = Arrangement.spacedBy(20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconTile(section.icon, section.tone)
            MqText(section.label, role = TextRole.Body, weight = FontWeight.SemiBold, maxLines = 1)
        }
    }
}

/** The header's height on TV, under the tab bar (600 from the top of the TV, as the web's). */
internal val TV_HEADER = 486.dp

/** The phone header's least height, art or none. */
internal val PHONE_HEADER = 340.dp

/** The content column on TV, and its section menu. */
internal val TV_COLUMN = 1600.dp
internal val TV_MENU = 420.dp
private val TV_AVATAR = 168.dp

/** The phone card's glass: `raised` at 82 percent over the art. */
internal val PHONE_CARD = Color(0xD12C2C2E)

internal const val SETTINGS_HEADER = "settings-header"
internal const val SETTINGS_ART = "settings-art"
internal const val SETTINGS_MENU = "settings-menu"
internal const val SETTINGS_LIST = "settings-list"
internal const val SETTINGS_PROFILE = "settings-profile"
