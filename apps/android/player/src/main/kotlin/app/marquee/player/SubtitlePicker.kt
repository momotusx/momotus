package app.marquee.player

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.model.SubtitleSize
import app.marquee.design.FieldInput
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LocalFocusRing
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.inverted
import app.marquee.design.marqueeFocus

// The subtitle picker (plans/player-polish.md 2.4, design/DECISION.md "Subtitle picker"): a search field, Off, the
// languages in the core's order with their flags, then a language's options. A phone shows the two levels in one
// sheet; a TV shows them side by side. Picking a language turns on its best option at once; the picker stays open.

/** The picker's place over the picture, as Apple's player draws its menus: rgba(30,30,32,.9), .94 on the phone. */
private val MENU_FILL = Color(0xE61E1E20)
private val SHEET_FILL = Color(0xF01E1E20)

/** The search field's fill, the segmented track's grey. */
private val FIELD_FILL = Color(0x3D767680)

/** An inset group inside a sheet, as the iPhone draws one ("#2C2C2E inside a sheet", design/DECISION.md). */
private val SHEET_GROUP = Color(0xFF2C2C2E)

const val SEARCH_LANGUAGES = "Search languages"

/** A language's flag (circle-flags, never emoji); with none, a neutral round mark with its two letters. */
@Composable
fun LanguageFlag(
    key: String,
    size: Dp,
    modifier: Modifier = Modifier,
) {
    val flag = flagFor(key)
    if (flag != null) {
        Image(painterResource(flag), contentDescription = null, modifier = modifier.size(size).clip(CircleShape))
    } else {
        Box(
            modifier.size(size).clip(CircleShape).background(Marquee.colors.raised),
            contentAlignment = Alignment.Center,
        ) {
            MqText(
                key.take(2).uppercase(),
                role = TextRole.Caption,
                color = Marquee.colors.muted,
                weight = FontWeight.SemiBold,
                maxLines = 1,
            )
        }
    }
}

/** The English name, and the language's own name quietly after it when both fit; else the own name is left out, never cut. */
@Composable
private fun LanguageName(
    language: SubtitleLanguage,
    color: Color,
    quiet: Color,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    Layout(
        content = {
            MqText(language.name, role = TextRole.Callout, color = color, maxLines = 1)
            language.nativeName?.let {
                MqText(
                    it,
                    role = if (tv) TextRole.Callout else TextRole.Caption,
                    color = quiet,
                    maxLines = 1,
                    softWrap = false,
                )
            }
        },
        modifier = modifier,
    ) { measurables, constraints ->
        val loose = constraints.copy(minWidth = 0)
        val name = measurables[0].measure(loose)
        val gap = (if (tv) 10.dp else 6.dp).roundToPx()
        val own =
            measurables.getOrNull(1)?.measure(loose.copy(maxWidth = Int.MAX_VALUE))?.takeIf {
                name.width + gap + it.width <= constraints.maxWidth
            }
        val width = if (own != null) name.width + gap + own.width else name.width
        layout(width.coerceAtLeast(constraints.minWidth), name.height) {
            name.placeRelative(0, 0)
            // On one baseline with the name.
            own?.placeRelative(name.width + gap, name[FirstBaseline] - own[FirstBaseline])
        }
    }
}

/** "Search languages": English name, own name or code. On a TV it is a button until selected, then the keyboard opens. */
@Composable
private fun SearchLanguages(
    query: String,
    onQuery: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    Row(
        modifier
            .fillMaxWidth()
            .height(if (tv) 60.dp else 36.dp)
            .marqueeFocus(Marquee.radius.pill, scale = 1f, forced = focused)
            .clip(RoundedCornerShape(if (tv) Marquee.radius.pill else 10.dp))
            .background(FIELD_FILL)
            .padding(horizontal = if (tv) 24.dp else 10.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 14.dp else 7.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Search, tint = Marquee.colors.dim, size = if (tv) 28.dp else 17.dp)
        FieldInput(
            query,
            onQuery,
            SEARCH_LANGUAGES,
            SEARCH_LANGUAGES,
            TextStyle(color = Marquee.colors.text, fontSize = Marquee.type.callout, fontFamily = MarqueeFonts.text),
            onFocus = { focused = it },
            modifier = Modifier.weight(1f),
        )
    }
}

/** What a language row says to a screen reader: its names and how many options it has. */
private fun spoken(language: SubtitleLanguage): String =
    listOfNotNull(
        language.name,
        language.nativeName,
        "${language.options.size} ${if (language.options.size == 1) "option" else "options"}",
    ).joinToString(", ")

// The phone: a full-height sheet at the right of the landscape player.

/**
 * The phone's picker, one level at a time in a sheet over the dimmed player: the languages, or the options of
 * [opened] (a language's key). The screen keeps [opened] so its Back steps out of the options first. [timing] is
 * this play's subtitle delay while a subtitle is on (the options level shows its row, and the profile's [textSize]
 * under it, changed through [onSize]); [note] what the core still
 * waits for or could not reach.
 */
@Composable
fun PhoneSubtitleSheet(
    rows: List<TrackChoice>,
    opened: String?,
    onOpen: (String?) -> Unit,
    onPick: (String) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
    note: String? = null,
    timing: Long? = null,
    onShift: (Long) -> Unit = {},
    textSize: SubtitleSize = SubtitleSize.Medium,
    onSize: (SubtitleSize) -> Unit = {},
) {
    val languages = remember(rows) { subtitleLanguages(rows) }
    val language = languages.firstOrNull { it.key == opened }
    Column(
        modifier
            .clip(RoundedCornerShape(26.dp))
            .background(SHEET_FILL)
            .testTag(SUBTITLE_SHEET_TAG)
            .semantics { paneTitle = "Subtitles" },
    ) {
        SheetHeader(language, onBack = { onOpen(null) }, onDone = onDone)
        if (language == null) {
            var query by rememberSaveable { mutableStateOf("") }
            val shown = filterLanguages(languages, query)
            SearchLanguages(query, { query = it }, Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
            LazyColumn(
                Modifier.weight(1f).padding(horizontal = 12.dp),
                contentPadding = PaddingValues(top = 6.dp, bottom = 12.dp),
            ) {
                item(key = TRACK_OFF) {
                    val off = rows.firstOrNull { it.id == TRACK_OFF }?.selected == true
                    PhoneRow(0, shown.size + 1, off, { onPick(TRACK_OFF) }, "Off") {
                        MqText("Off", Modifier.weight(1f), role = TextRole.Callout, maxLines = 1)
                        if (off) Checked()
                    }
                }
                items(shown, key = { "lang:${it.key}" }) { lang ->
                    PhoneLanguageRow(lang, shown.indexOf(lang) + 1, shown.size + 1, onPick, onOpen)
                }
                if (note != null) {
                    item(key = "note") {
                        MqText(note, Modifier.padding(16.dp), role = TextRole.Caption, color = Marquee.colors.dim)
                    }
                }
            }
        } else {
            LazyColumn(
                Modifier.weight(1f).padding(horizontal = 12.dp),
                contentPadding = PaddingValues(bottom = 12.dp),
            ) {
                items(language.options, key = { it.id }) { option ->
                    val i = language.options.indexOf(option)
                    PhoneRow(i, language.options.size, option.selected, { onPick(option.id) }, spokenOption(option)) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            MqText(option.name, role = TextRole.Callout, maxLines = 1)
                            OptionLine(option, Marquee.colors.dim)
                        }
                        if (option.selected) Checked()
                    }
                }
            }
            val line = Marquee.colors.line
            Column(
                Modifier
                    .drawBehind { drawLine(line, Offset.Zero, Offset(size.width, 0f), 1f) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (timing != null) TimingRow(timing, onShift)
                SizeRow(textSize, onSize)
            }
        }
    }
}

/** The sheet's title bar: "Subtitles" or the language, Done at the right, and the way back to the languages. */
@Composable
private fun SheetHeader(
    language: SubtitleLanguage?,
    onBack: () -> Unit,
    onDone: () -> Unit,
) {
    val key = Marquee.colors.key
    Box(Modifier.fillMaxWidth().height(48.dp)) {
        if (language != null) {
            Row(
                Modifier
                    .align(Alignment.CenterStart)
                    .marqueeFocus(Marquee.radius.m, scale = 1f)
                    .semantics { contentDescription = "Back to languages" }
                    .focusClickable(onBack)
                    .heightIn(min = 44.dp)
                    .padding(start = 4.dp, end = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MqIcon(Icons.ChevronLeft, tint = key, size = 22.dp)
                MqText("Languages", role = TextRole.Headline, color = key, maxLines = 1)
            }
        }
        MqText(
            language?.name ?: "Subtitles",
            Modifier.align(Alignment.Center).semantics { heading() },
            role = TextRole.Headline,
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
        MqText(
            "Done",
            Modifier
                .align(Alignment.CenterEnd)
                .marqueeFocus(Marquee.radius.m, scale = 1f)
                .focusClickable(onDone)
                .heightIn(min = 44.dp)
                .padding(horizontal = 14.dp, vertical = 11.dp),
            role = TextRole.Headline,
            color = key,
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

/** A language: tapping it turns its best option on; the chevron opens its options. */
@Composable
private fun PhoneLanguageRow(
    lang: SubtitleLanguage,
    index: Int,
    count: Int,
    onPick: (String) -> Unit,
    onOpen: (String?) -> Unit,
) {
    val colors = Marquee.colors
    PhoneRow(index, count, lang.chosen, { onPick(lang.best.id) }, spoken(lang), end = {
        Box(
            Modifier
                .size(44.dp)
                .marqueeFocus(Marquee.radius.m, scale = 1f)
                .semantics { contentDescription = "${lang.name} options" }
                .focusClickable({ onOpen(lang.key) }),
            contentAlignment = Alignment.Center,
        ) { MqIcon(Icons.ChevronRight, tint = colors.text.copy(alpha = 0.4f), size = 15.dp) }
    }) {
        LanguageFlag(lang.key, 24.dp)
        LanguageName(lang, colors.text, colors.dim, Modifier.weight(1f))
        MqText("${lang.options.size}", role = TextRole.Callout, color = colors.dim, maxLines = 1)
        if (lang.chosen) Checked()
    }
}

/** The key-blue check of the chosen row, as iOS lists draw it. */
@Composable
private fun Checked() = MqIcon(Icons.Check, tint = Marquee.colors.key, size = 17.dp)

/**
 * One row of the sheet's inset group: [index] of [count] rounds the group's ends and draws the hairline between
 * rows. The row's own content is one radio button; [end] sits beside it as its own control.
 */
@Composable
private fun PhoneRow(
    index: Int,
    count: Int,
    selected: Boolean,
    onClick: () -> Unit,
    spoken: String,
    end: (@Composable () -> Unit)? = null,
    content: @Composable RowScope.() -> Unit,
) {
    val r = 16.dp
    val shape =
        RoundedCornerShape(
            topStart = if (index == 0) r else 0.dp,
            topEnd = if (index == 0) r else 0.dp,
            bottomStart = if (index == count - 1) r else 0.dp,
            bottomEnd = if (index == count - 1) r else 0.dp,
        )
    val line = Marquee.colors.line
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(SHEET_GROUP)
            .drawBehind {
                if (index > 0) drawLine(line, Offset(16.dp.toPx(), 0f), Offset(size.width, 0f), 1f)
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .heightIn(min = 44.dp)
                .marqueeFocus(Marquee.radius.m, scale = 1f)
                // The click first: what is cleared after it is the row's words, said once as [spoken].
                .focusClickable(onClick, Role.RadioButton)
                .clearAndSetSemantics {
                    this.selected = selected
                    contentDescription = spoken
                }.padding(start = 14.dp, end = if (end == null) 14.dp else 4.dp, top = 8.dp, bottom = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
        end?.invoke()
    }
}

/**
 * An option's quiet line, its release text cut to what the row's width holds ([optionLine]), measured in the
 * caption's own type: two lines at most.
 */
@Composable
private fun OptionLine(
    option: TrackChoice,
    color: Color,
    weight: FontWeight? = null,
) {
    val measurer = rememberTextMeasurer()
    val style = TextStyle(fontSize = Marquee.type.caption, fontFamily = MarqueeFonts.text, fontWeight = weight)
    BoxWithConstraints {
        val width = constraints.maxWidth
        val line =
            remember(option, width, style) {
                optionLine(option) { measurer.measure(it, style, softWrap = false, maxLines = 1).size.width <= width }
            }
        MqText(line, role = TextRole.Caption, color = color, weight = weight, maxLines = 2)
    }
}

/** What an option row says to a screen reader: its name, then its line. */
private fun spokenOption(option: TrackChoice): String =
    listOf(option.name, optionLine(option) { true }).filter { it.isNotEmpty() }.joinToString(", ")

// The TV: two columns in one panel, the languages and the focused language's options.

/**
 * The TV's picker: the languages at the left with the search field and Off, the focused language's options at the
 * right, where Right moves focus and Left comes back. Focus opens on the chosen language (or Off). The TV has no
 * key color: the chosen row is the grey fill with a white check, the focused row turns white.
 */
@Composable
fun TvSubtitlePicker(
    rows: List<TrackChoice>,
    onPick: (String) -> Unit,
    modifier: Modifier = Modifier,
    note: String? = null,
    timing: Long? = null,
    onShift: (Long) -> Unit = {},
    textSize: SubtitleSize = SubtitleSize.Medium,
    onSize: (SubtitleSize) -> Unit = {},
) {
    val languages = remember(rows) { subtitleLanguages(rows) }
    var query by rememberSaveable { mutableStateOf("") }
    val shown = filterLanguages(languages, query)
    val chosen = languages.firstOrNull { it.chosen }
    var focusedKey by remember { mutableStateOf(chosen?.key) }
    // The right column follows the focused language; with focus on Off or the field, the last one.
    val preview = languages.firstOrNull { it.key == focusedKey } ?: chosen ?: languages.firstOrNull()
    val requesters = remember { HashMap<String, FocusRequester>() }
    val requester = { key: String -> requesters.getOrPut(key) { FocusRequester() } }
    val off = remember { FocusRequester() }
    val into = remember { FocusRequester() }
    val start = (chosen?.let { shown.indexOf(it) + 1 } ?: 0).coerceAtLeast(0)
    val list = rememberLazyListState(initialFirstVisibleItemIndex = (start - 2).coerceAtLeast(0))
    LaunchedEffect(Unit) {
        withFrameNanos { }
        withFrameNanos { }
        runCatching { (chosen?.let { requester(it.key) } ?: off).requestFocus() }
    }
    // The picker is v3 throughout (its frames): its buttons and field invert on focus, not the player's ring.
    CompositionLocalProvider(LocalFocusRing provides false) {
        Row(
            modifier
                .width(1160.dp)
                .height(736.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(MENU_FILL)
                .testTag(SUBTITLE_PICKER_TAG)
                .semantics { paneTitle = "Subtitles" },
        ) {
            Column(Modifier.width(500.dp).fillMaxHeight().padding(horizontal = 16.dp, vertical = 24.dp)) {
                Heading("Subtitles")
                SearchLanguages(
                    query,
                    { query = it },
                    Modifier.padding(bottom = 10.dp).focusProperties {
                        up = FocusRequester.Cancel
                        left = FocusRequester.Cancel
                        right = FocusRequester.Cancel
                    },
                )
                LazyColumn(
                    Modifier.weight(1f),
                    state = list,
                    // Room for the focused row's lift.
                    contentPadding = PaddingValues(vertical = 6.dp),
                ) {
                    item(key = TRACK_OFF) {
                        val on = rows.firstOrNull { it.id == TRACK_OFF }?.selected == true
                        TvRow(
                            on,
                            { onPick(TRACK_OFF) },
                            "Off",
                            Modifier.focusRequester(off).focusProperties {
                                right =
                                    FocusRequester.Cancel
                            },
                        ) {
                            Spacer(Modifier.width(40.dp))
                            MqText(
                                "Off",
                                Modifier.weight(1f),
                                role = TextRole.Callout,
                                color = it.text,
                                weight = it.weight,
                                maxLines = 1,
                            )
                            TvCheck(on, it)
                        }
                    }
                    items(shown, key = { "lang:${it.key}" }) { lang ->
                        TvRow(
                            lang.chosen,
                            { onPick(lang.best.id) },
                            spoken(lang),
                            Modifier
                                .focusRequester(requester(lang.key))
                                .onFocusChanged { if (it.isFocused) focusedKey = lang.key }
                                .focusProperties {
                                    right = into
                                    if (lang == shown.last()) down = FocusRequester.Cancel
                                },
                        ) {
                            LanguageFlag(lang.key, 40.dp)
                            LanguageName(lang, it.text, it.quiet, Modifier.weight(1f))
                            MqText("${lang.options.size}", role = TextRole.Callout, color = it.quiet, maxLines = 1)
                            TvCheck(lang.chosen, it)
                        }
                    }
                    if (note != null) {
                        item(key = "note") {
                            MqText(note, Modifier.padding(16.dp), role = TextRole.Caption, color = Marquee.colors.dim)
                        }
                    }
                }
            }
            Box(Modifier.width(1.dp).fillMaxHeight().background(Marquee.colors.line))
            Column(Modifier.weight(1f).fillMaxHeight().padding(horizontal = 16.dp, vertical = 24.dp)) {
                if (preview != null) {
                    Heading(preview.name)
                    val target = preview.options.firstOrNull { it.selected } ?: preview.options.first()
                    val back = requester(preview.key)
                    Column(
                        Modifier
                            .weight(
                                1f,
                                fill = false,
                            ).verticalScroll(rememberScrollState())
                            .padding(vertical = 6.dp),
                    ) {
                        preview.options.forEach { option ->
                            TvRow(
                                option.selected,
                                { onPick(option.id) },
                                spokenOption(option),
                                Modifier
                                    .then(if (option == target) Modifier.focusRequester(into) else Modifier)
                                    .focusProperties {
                                        left = back
                                        right = FocusRequester.Cancel
                                        if (option == preview.options.first()) up = FocusRequester.Cancel
                                    },
                                top = true,
                            ) {
                                Column(Modifier.weight(1f)) {
                                    MqText(
                                        option.name,
                                        role = TextRole.Callout,
                                        color = it.text,
                                        weight = it.weight,
                                        maxLines = 1,
                                    )
                                    OptionLine(option, it.quiet, FontWeight.Medium)
                                }
                                TvCheck(option.selected, it)
                            }
                        }
                    }
                    if (timing != null) {
                        TimingRow(
                            timing,
                            onShift,
                            Modifier.padding(start = 16.dp, top = 12.dp),
                        )
                    }
                    SizeRow(
                        textSize,
                        onSize,
                        Modifier.padding(start = 16.dp, top = 12.dp),
                        first =
                            Modifier.focusProperties {
                                left = back
                                down = FocusRequester.Cancel
                            },
                    )
                }
            }
        }
    }
}

/** A column's heading: 64 percent, as the frames' section titles. */
@Composable
private fun Heading(text: String) =
    MqText(
        text,
        Modifier.padding(start = 16.dp, bottom = 12.dp).semantics { heading() },
        role = TextRole.Callout,
        color = Marquee.colors.dim,
        weight = FontWeight.SemiBold,
        maxLines = 1,
    )

/** The words' colors and weight on a TV row: white on the menu, black on the focused white row. */
private class RowInk(
    val text: Color,
    val quiet: Color,
    val weight: FontWeight,
)

/** The white check of the chosen row on TV (black on the focused row); room for it otherwise, so the counts align. */
@Composable
private fun TvCheck(
    on: Boolean,
    ink: RowInk,
) = Box(Modifier.size(28.dp)) { if (on) MqIcon(Icons.Check, tint = ink.text, size = 28.dp) }

/**
 * One TV row: the chosen one on the grey fill, the focused one white with black words and a small lift (tvOS
 * inverts a focused row, design/DECISION.md "TV focus"). Its content is told the words' [RowInk].
 */
@Composable
private fun TvRow(
    on: Boolean,
    onClick: () -> Unit,
    spoken: String,
    modifier: Modifier,
    top: Boolean = false,
    content: @Composable RowScope.(RowInk) -> Unit,
) {
    val colors = Marquee.colors
    var focused by remember { mutableStateOf(false) }
    val lift by animateFloatAsState(if (focused) 1f else 0f, tween(Marquee.focusMs), label = "lift")
    val ink =
        when {
            focused -> colors.inverted().let { RowInk(it.text, it.dim, FontWeight.Medium) }
            on -> RowInk(Color.White, colors.dim, FontWeight.SemiBold)
            else -> RowInk(colors.text, colors.dim, FontWeight.Medium)
        }
    Row(
        modifier
            .fillMaxWidth()
            .onFocusChanged { focused = it.isFocused }
            .graphicsLayer {
                val s = 1f + 0.03f * lift
                scaleX = s
                scaleY = s
            }.clip(RoundedCornerShape(16.dp))
            .background(
                if (focused) {
                    colors.primary
                } else if (on) {
                    colors.raised
                } else {
                    Color.Transparent
                },
            )
            // The click first: what is cleared after it is the row's words, said once as [spoken].
            .focusClickable(onClick, Role.RadioButton)
            .clearAndSetSemantics {
                selected = on
                contentDescription = spoken
            }.heightIn(min = 60.dp)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = if (top) Alignment.Top else Alignment.CenterVertically,
    ) { content(ink) }
}

/** The pickers, for tests. */
const val SUBTITLE_SHEET_TAG = "subtitle-sheet"
const val SUBTITLE_PICKER_TAG = "subtitle-picker"
