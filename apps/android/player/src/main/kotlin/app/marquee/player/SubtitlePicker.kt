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
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
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
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
import app.marquee.design.FieldInput
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LocalOverPicture
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.Spinner
import app.marquee.design.TextRole
import app.marquee.design.Tokens
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

/** A language's flag (circle-flags, never emoji); with none, a neutral round mark with the language glyph
 * ([flagGlyph]) or its two letters. */
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
            if (flagGlyph(key)) {
                MqIcon(Icons.Languages, tint = Marquee.colors.muted, size = size * 0.56f)
            } else {
                MqText(
                    flagLetters(key),
                    role = TextRole.Caption,
                    color = Marquee.colors.muted,
                    weight = FontWeight.SemiBold,
                    maxLines = 1,
                )
            }
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
 * the chosen track's subtitle delay while a subtitle is on (the options level shows its row, and the profile's
 * [textSize] under it, changed through [onSize]); [note] what the core still waits for or could not reach.
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
    onDelay: (Long) -> Unit = {},
    textSize: SubtitleSize = SubtitleSize.Medium,
    onSize: (SubtitleSize) -> Unit = {},
    /** The second line while one can be on (the switch shows then); null otherwise. */
    second: SecondLine? = null,
    /** The Second line tab shows (with [second]), and the switch's change of it. */
    secondTab: Boolean = false,
    onTab: (Boolean) -> Unit = {},
) {
    val tab = Tab.of(rows, second, secondTab)
    val language = tab.languages.firstOrNull { it.key == opened && tab.taken(it) == null }
    Column(
        modifier
            .clip(RoundedCornerShape(26.dp))
            .background(SHEET_FILL)
            .testTag(SUBTITLE_SHEET_TAG)
            .semantics { paneTitle = "Subtitles" },
    ) {
        SheetHeader(language, onBack = { onOpen(null) }, onDone = onDone)
        // Held over both levels: back from a language, the search and the place in the list are as they were.
        var query by rememberSaveable { mutableStateOf("") }
        val list = rememberLazyListState()
        if (language == null) {
            val shown = filterLanguages(tab.languages, query)
            if (second != null) {
                LineSwitch(
                    tab.second,
                    {
                        onOpen(null)
                        onTab(it)
                    },
                    Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                )
            }
            SearchLanguages(query, { query = it }, Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
            val spokenRow = tab.second && query.isBlank()
            val count = shown.size + 1 + if (spokenRow) 1 else 0
            LazyColumn(
                Modifier.weight(1f).padding(horizontal = 12.dp),
                state = list,
                contentPadding = PaddingValues(top = 6.dp, bottom = 12.dp),
            ) {
                item(key = TRACK_OFF) {
                    PhoneRow(0, count, tab.off, { tab.pick(TRACK_OFF, onPick) }, "Off") {
                        MqText("Off", Modifier.weight(1f), role = TextRole.Callout, maxLines = 1)
                        if (tab.off) Checked()
                    }
                }
                if (spokenRow && second != null) {
                    item(key = "spoken") { PhoneSpokenRow(second, tab, count, onOpen) }
                }
                items(shown, key = { "lang:${it.key}" }) { lang ->
                    val index = shown.indexOf(lang) + 1 + if (spokenRow) 1 else 0
                    PhoneLanguageRow(
                        lang,
                        index,
                        count,
                        tab.chosen(lang),
                        tab.taken(lang),
                        { tab.pick(it, onPick) },
                        onOpen,
                    )
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
                    PhoneRow(
                        i,
                        language.options.size,
                        option.selected,
                        { tab.pick(option.id, onPick) },
                        spokenOption(option),
                    ) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            MqText(option.name, role = TextRole.Callout, maxLines = 1)
                            OptionLine(option, Marquee.colors.dim)
                        }
                        if (option.selected) Checked()
                    }
                }
                // The tuning foot scrolls with the options: a landscape phone cannot hold four fixed rows.
                item(key = "tune") {
                    val line = Marquee.colors.line
                    Column(
                        Modifier
                            .padding(top = 12.dp)
                            .drawBehind { drawLine(line, Offset.Zero, Offset(size.width, 0f), 1f) }
                            .padding(horizontal = 4.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        if (tab.second && second != null) {
                            second.timing?.let { TimingRow(it, second.onDelay) }
                            SizeRow(textSize, onSize)
                            LayoutRow(second.layout, second.onLayout)
                            PausedRow(second.onPause, second.onPauseChange)
                        } else {
                            if (timing != null) TimingRow(timing, onDelay)
                            SizeRow(textSize, onSize)
                        }
                    }
                }
            }
        }
    }
}

/**
 * The second subtitle line in the picker (plans/dual-subtitles.md 5): the Second line tab's [rows] (Off and the core's
 * tracks), the first line's language [firstKey] (which that tab disables) and the second's [secondKey] (which the First
 * line tab disables), the [spoken] language and whether its row is the choice, the line's [timing] while one is on,
 * and the profile's [layout] and [onPause].
 */
class SecondLine(
    val rows: List<TrackChoice>,
    val firstKey: String?,
    val secondKey: String?,
    val spoken: String?,
    val spokenOn: Boolean,
    /** The core still lists subtitles: the spoken language may yet be known. */
    val pending: Boolean,
    val timing: Long?,
    val layout: SubtitleLayout,
    val onPause: Boolean,
    val onPick: (String) -> Unit,
    /** The Spoken language row: its best track, or the profile's choice with none here. */
    val onSpoken: () -> Unit,
    val onDelay: (Long) -> Unit,
    val onLayout: (SubtitleLayout) -> Unit,
    val onPauseChange: (Boolean) -> Unit,
)

/**
 * The picker's second line from the controller; null while none can be on ([PlaybackController.secondOffered]).
 * [onLayout] and [onPause] write the profile's settings, [onSpokenKept] keeps the Spoken language as its second line.
 */
fun PlaybackController.secondLine(
    onLayout: (SubtitleLayout) -> Unit,
    onPause: (Boolean) -> Unit,
    onSpokenKept: () -> Unit,
): SecondLine? {
    if (!secondOffered) return null
    val rows = secondText
    return SecondLine(
        rows = rows,
        firstKey = firstKey,
        secondKey = secondKey,
        spoken = spokenLanguage,
        spokenOn = spokenOn,
        pending = subtitlesPending,
        timing = secondDelayMs.takeIf { rows.any { it.selected && it.id != TRACK_OFF } },
        layout = subtitleLayout,
        onPause = secondOnPause,
        onPick = { selectSecond(it) },
        onSpoken = {
            val best = subtitleLanguages(rows).firstOrNull { samePrimaryLanguage(it.key, spokenLanguage) }?.best
            if (best != null) {
                selectSecond(best.id, spoken = true)
            } else {
                keepSpoken()
                onSpokenKept()
            }
        },
        onDelay = ::setSecondDelay,
        onLayout = onLayout,
        onPauseChange = onPause,
    )
}

/**
 * What the picker shows on its tab: the First line tab is today's picker with the second line's language taken; the
 * Second line tab lists the core's languages with the first line's taken, and the Spoken language row.
 */
private class Tab(
    val second: Boolean,
    val languages: List<SubtitleLanguage>,
    /** The other line's language, which this tab cannot pick. */
    private val other: String?,
    private val line: SecondLine?,
    rows: List<TrackChoice>,
) {
    /** Off is the choice: no track on this tab's line, and on the Second line tab not the Spoken language row. */
    val off: Boolean = rows.firstOrNull { it.id == TRACK_OFF }?.selected == true && !spokenOn

    val spokenOn: Boolean get() = second && line?.spokenOn == true

    /** A language row's check: the Spoken language row takes it while it is the choice. */
    fun chosen(language: SubtitleLanguage): Boolean = language.chosen && !spokenOn

    /** The words a row of the other line's language reads in place of its own name; null for a row that can be picked. */
    fun taken(language: SubtitleLanguage): String? =
        when {
            !samePrimaryLanguage(language.key, other) -> null
            second -> StreamChoiceCopy.ON_FIRST_LINE
            else -> StreamChoiceCopy.ON_SECOND_LINE
        }

    /** A row picked on this tab goes to its line. */
    fun pick(
        id: String,
        first: (String) -> Unit,
    ) = if (second && line != null) line.onPick(id) else first(id)

    companion object {
        fun of(
            rows: List<TrackChoice>,
            line: SecondLine?,
            secondTab: Boolean,
        ): Tab {
            val two = secondTab && line != null
            val tabRows = if (two) line!!.rows else rows
            return Tab(two, subtitleLanguages(tabRows), if (two) line!!.firstKey else line?.secondKey, line, tabRows)
        }
    }
}

/** First line and Second line (plans/dual-subtitles.md 5.1): two segments, the tab shown filled. */
@Composable
private fun LineSwitch(
    second: Boolean,
    onTab: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    /** TV: each segment's own focus requester and properties (Left and Right stop at the ends). */
    segment: (Boolean) -> Modifier = { Modifier },
) {
    val tv = Marquee.form == Form.Tv
    Row(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(FIELD_FILL)
            .selectableGroup()
            .padding(SWITCH_PAD),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        for (two in listOf(false, true)) {
            var focused by remember { mutableStateOf(false) }
            val on = two == second
            Box(
                segment(two)
                    .weight(1f)
                    .onFocusChanged { focused = it.isFocused }
                    .marqueeFocus(Marquee.radius.pill, scale = 1f)
                    .clip(RoundedCornerShape(Marquee.radius.pill))
                    .background(
                        when {
                            tv && focused -> Marquee.colors.primary
                            on -> Tokens.selectedFill
                            else -> Color.Transparent
                        },
                    ).focusClickable({ if (!on) onTab(two) }, Role.Tab)
                    .semantics { selected = on }
                    .heightIn(min = if (tv) 52.dp else 48.dp),
                contentAlignment = Alignment.Center,
            ) {
                MqText(
                    if (two) StreamChoiceCopy.SECOND_LINE else StreamChoiceCopy.FIRST_LINE,
                    role = TextRole.Callout,
                    color =
                        when {
                            tv && focused -> Marquee.colors.onPrimary
                            on -> Color.White
                            else -> Marquee.colors.dim
                        },
                    weight = FontWeight.SemiBold,
                    maxLines = 1,
                )
            }
        }
    }
}

private val SWITCH_PAD = 2.dp

/** A row of the other line's language: its flag drawn at half strength, as its words are quiet. */
private const val TAKEN_ALPHA = 0.5f

/** The TV's focus on the Spoken language row: the right column shows its language's options, Left comes back to it. */
private const val SPOKEN_ROW = "spoken:row"

/**
 * The Spoken language row: the audio's language beside the words ("Spoken language, French"), pending while the
 * core still lists, "not known for this video" once it settled with none; "On the first line" when the audio's language
 * is the first line's. Its chevron opens the spoken language's tracks.
 */
@Composable
private fun PhoneSpokenRow(
    line: SecondLine,
    tab: Tab,
    count: Int,
    onOpen: (String?) -> Unit,
) {
    val spoken = spokenLanguage(line, tab)
    val colors = Marquee.colors
    PhoneRow(
        1,
        count,
        line.spokenOn,
        line.onSpoken,
        spokenWords(line, spoken),
        enabled = spoken.taken == null,
        end =
            spoken.language?.takeIf { spoken.taken == null }?.let { lang ->
                {
                    Box(
                        Modifier
                            .size(48.dp)
                            .marqueeFocus(Marquee.radius.m, scale = 1f)
                            .semantics { contentDescription = "${StreamChoiceCopy.SPOKEN_LANGUAGE} options" }
                            .focusClickable({ onOpen(lang.key) }),
                        contentAlignment = Alignment.Center,
                    ) { MqIcon(Icons.ChevronRight, tint = colors.text.copy(alpha = 0.4f), size = 15.dp) }
                }
            },
    ) {
        val quiet = if (spoken.taken != null) colors.dim else colors.text
        line.spoken?.let { LanguageFlag(languageKey(it), 24.dp) } ?: Spacer(Modifier.size(24.dp))
        Row(
            Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqText(StreamChoiceCopy.SPOKEN_LANGUAGE, role = TextRole.Callout, color = quiet, maxLines = 1)
            SpokenDetail(spoken, line.pending, colors.dim)
        }
        if (line.spokenOn) Checked()
    }
}

/** The spoken language's own row in this tab's list (null while not known or with no track of it), and its taken words. */
private class Spoken(
    val language: SubtitleLanguage?,
    val name: String?,
    val taken: String?,
)

private fun spokenLanguage(
    line: SecondLine,
    tab: Tab,
): Spoken {
    val key = line.spoken?.let(::languageKey)
    val language = tab.languages.firstOrNull { samePrimaryLanguage(it.key, key) }
    val taken = StreamChoiceCopy.ON_FIRST_LINE.takeIf { samePrimaryLanguage(key, line.firstKey) }
    return Spoken(language, key?.let { language?.name ?: languageName(it) }, taken)
}

/** "French" after the words, the taken words, the pending mark while the core lists, else "not known for this video". */
@Composable
private fun SpokenDetail(
    spoken: Spoken,
    pending: Boolean,
    color: Color,
) {
    val words = spoken.taken ?: spoken.name ?: StreamChoiceCopy.SPOKEN_NOT_KNOWN.takeUnless { pending }
    if (words != null) {
        MqText(words, role = TextRole.Caption, color = color, maxLines = 1)
    } else {
        Spinner(size = if (Marquee.form == Form.Tv) 18.dp else 12.dp, color = color)
    }
}

/** What the Spoken language row says to a screen reader. */
private fun spokenWords(
    line: SecondLine,
    spoken: Spoken,
): String =
    listOfNotNull(
        StreamChoiceCopy.SPOKEN_LANGUAGE,
        spoken.taken ?: spoken.name ?: if (line.pending) "still searching" else StreamChoiceCopy.SPOKEN_NOT_KNOWN,
    ).joinToString(", ")

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
                    .heightIn(min = 48.dp)
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
                .heightIn(min = 48.dp)
                .padding(horizontal = 14.dp, vertical = 11.dp),
            role = TextRole.Headline,
            color = key,
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

/**
 * A language: tapping it turns its best option on; the chevron opens its options. One the other line shows ([taken])
 * reads so in place of its own name, and is neither picked nor opened here.
 */
@Composable
private fun PhoneLanguageRow(
    lang: SubtitleLanguage,
    index: Int,
    count: Int,
    chosen: Boolean,
    taken: String?,
    onPick: (String) -> Unit,
    onOpen: (String?) -> Unit,
) {
    val colors = Marquee.colors
    val row = if (taken != null) lang.copy(nativeName = taken) else lang
    val end: @Composable () -> Unit = {
        Box(
            Modifier
                .size(48.dp)
                .marqueeFocus(Marquee.radius.m, scale = 1f)
                .semantics { contentDescription = "${lang.name} options" }
                .focusClickable({ onOpen(lang.key) }),
            contentAlignment = Alignment.Center,
        ) { MqIcon(Icons.ChevronRight, tint = colors.text.copy(alpha = 0.4f), size = 15.dp) }
    }
    PhoneRow(
        index,
        count,
        chosen,
        { onPick(lang.best.id) },
        spoken(row),
        enabled = taken == null,
        end = end.takeIf { taken == null },
    ) {
        LanguageFlag(lang.key, 24.dp, if (taken != null) Modifier.alpha(TAKEN_ALPHA) else Modifier)
        LanguageName(row, if (taken != null) colors.dim else colors.text, colors.dim, Modifier.weight(1f))
        MqText("${lang.options.size}", role = TextRole.Callout, color = colors.dim, maxLines = 1)
        if (chosen) Checked()
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
    enabled: Boolean = true,
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
                .heightIn(min = 48.dp)
                .marqueeFocus(Marquee.radius.m, scale = 1f)
                // The click first: what is cleared after it is the row's words, said once as [spoken].
                .focusClickable(onClick, Role.RadioButton, enabled = enabled)
                .clearAndSetSemantics {
                    this.selected = selected
                    contentDescription = spoken
                    if (!enabled) disabled()
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
    onDelay: (Long) -> Unit = {},
    textSize: SubtitleSize = SubtitleSize.Medium,
    onSize: (SubtitleSize) -> Unit = {},
    /** The second line while one can be on (the switch shows then); null otherwise. */
    second: SecondLine? = null,
    /** The Second line tab shows (with [second]), and the switch's change of it. */
    secondTab: Boolean = false,
    onTab: (Boolean) -> Unit = {},
) {
    val tab = Tab.of(rows, second, secondTab)
    var query by rememberSaveable { mutableStateOf("") }
    // The switch's segments, kept over a change of tab so focus can go back to them.
    val segments = remember { listOf(FocusRequester(), FocusRequester()) }
    // The picker is its own panel (its frames): its buttons take the plain fills, not the over-the-picture one.
    CompositionLocalProvider(LocalOverPicture provides false) {
        Row(
            modifier
                .width(1160.dp)
                .height(736.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(MENU_FILL)
                .testTag(SUBTITLE_PICKER_TAG)
                .semantics { paneTitle = "Subtitles" },
        ) {
            // A tab's rows, focus requesters and list are its own: OK on the switch rebuilds them, and focus goes to
            // the new tab's chosen row (plans/dual-subtitles.md 5.3).
            key(tab.second) {
                TvColumns(
                    tab,
                    query,
                    { query = it },
                    onPick,
                    note,
                    timing,
                    onDelay,
                    textSize,
                    onSize,
                    second,
                    segments,
                ) {
                    if (second != null) {
                        LineSwitch(tab.second, onTab, Modifier.padding(bottom = 10.dp)) { two ->
                            Modifier.focusRequester(segments[if (two) 1 else 0]).focusProperties {
                                // The ends stop a remote: focus never leaves the panel.
                                if (!two) left = FocusRequester.Cancel
                                if (two) right = FocusRequester.Cancel
                                up = FocusRequester.Cancel
                            }
                        }
                    }
                }
            }
        }
    }
}

/** The TV picker's two columns for one tab: the languages with the switch ([lineSwitch]) and search, the options. */
@Composable
private fun RowScope.TvColumns(
    tab: Tab,
    query: String,
    onQuery: (String) -> Unit,
    onPick: (String) -> Unit,
    note: String?,
    timing: Long?,
    onDelay: (Long) -> Unit,
    textSize: SubtitleSize,
    onSize: (SubtitleSize) -> Unit,
    second: SecondLine?,
    segments: List<FocusRequester>,
    lineSwitch: @Composable () -> Unit,
) {
    val languages = tab.languages
    val shown = filterLanguages(languages, query)
    val chosen = languages.firstOrNull { tab.chosen(it) }
    val spokenRow = tab.second && second != null && query.isBlank()
    val spoken = second?.let { spokenLanguage(it, tab) }
    var focusedKey by remember { mutableStateOf(chosen?.key) }
    // The right column follows the focused language; with focus on Off or the field, the last one. The other line's
    // language never shows its options here; the Spoken language row shows its language's, none with no track of it.
    val preview =
        if (focusedKey == SPOKEN_ROW) {
            spoken?.language
        } else {
            languages.firstOrNull { it.key == focusedKey && tab.taken(it) == null }
                ?: chosen
                ?: spoken?.language?.takeIf { tab.spokenOn && spoken.taken == null }
                ?: languages.firstOrNull { tab.taken(it) == null }
        }
    // No language here can be picked (each is the first line's): the Second line tab's foot stands alone at the right,
    // where Right from Off or the Spoken language row reaches it.
    val footOnly = preview == null && tab.second && second != null && languages.none { tab.taken(it) == null }
    val requesters = remember { HashMap<String, FocusRequester>() }
    val requester = { key: String -> requesters.getOrPut(key) { FocusRequester() } }
    val off = remember { FocusRequester() }
    val spokenFocus = remember { FocusRequester() }
    val into = remember { FocusRequester() }
    val start = (chosen?.let { shown.indexOf(it) + 1 + if (spokenRow) 1 else 0 } ?: 0).coerceAtLeast(0)
    val list = rememberLazyListState(initialFirstVisibleItemIndex = (start - 2).coerceAtLeast(0))
    LaunchedEffect(Unit) {
        withFrameNanos { }
        withFrameNanos { }
        val target =
            when {
                spokenRow && tab.spokenOn && spoken?.taken == null -> spokenFocus
                chosen != null -> requester(chosen.key)
                else -> off
            }
        runCatching { target.requestFocus() }
    }
    val lastOpen = shown.lastOrNull { tab.taken(it) == null }
    Column(Modifier.width(500.dp).fillMaxHeight().padding(horizontal = 16.dp, vertical = 24.dp)) {
        Heading("Subtitles")
        lineSwitch()
        SearchLanguages(
            query,
            onQuery,
            Modifier.padding(bottom = 10.dp).focusProperties {
                // Up reaches the switch (its tab's segment) when it shows.
                up = if (second != null) segments[if (tab.second) 1 else 0] else FocusRequester.Cancel
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
                TvRow(
                    tab.off,
                    { tab.pick(TRACK_OFF, onPick) },
                    "Off",
                    // Focus stays in the panel while it is open: the controls under it take nothing.
                    Modifier.focusRequester(off).focusProperties {
                        left = FocusRequester.Cancel
                        right = if (footOnly) into else FocusRequester.Cancel
                        if (lastOpen == null && !spokenRow) down = FocusRequester.Cancel
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
                    TvCheck(tab.off, it)
                }
            }
            if (spokenRow && second != null && spoken != null) {
                item(key = "spoken") {
                    TvRow(
                        second.spokenOn,
                        second.onSpoken,
                        spokenWords(second, spoken),
                        Modifier
                            .focusRequester(spokenFocus)
                            .onFocusChanged { if (it.isFocused) focusedKey = SPOKEN_ROW }
                            .focusProperties {
                                left = FocusRequester.Cancel
                                right = if (spoken.language != null || footOnly) into else FocusRequester.Cancel
                                if (lastOpen == null) down = FocusRequester.Cancel
                            },
                        enabled = spoken.taken == null,
                    ) {
                        second.spoken?.let { tag -> LanguageFlag(languageKey(tag), 40.dp) }
                            ?: Spacer(Modifier.width(40.dp))
                        Row(
                            Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            MqText(
                                StreamChoiceCopy.SPOKEN_LANGUAGE,
                                role = TextRole.Callout,
                                color = it.text,
                                weight = it.weight,
                                maxLines = 1,
                            )
                            SpokenDetail(spoken, second.pending, it.quiet)
                        }
                        TvCheck(second.spokenOn, it)
                    }
                }
            }
            items(shown, key = { "lang:${it.key}" }) { lang ->
                val taken = tab.taken(lang)
                val row = if (taken != null) lang.copy(nativeName = taken) else lang
                val on = tab.chosen(lang)
                TvRow(
                    on,
                    { tab.pick(lang.best.id, onPick) },
                    spoken(row),
                    Modifier
                        .focusRequester(requester(lang.key))
                        .onFocusChanged { if (it.isFocused) focusedKey = lang.key }
                        .focusProperties {
                            left = FocusRequester.Cancel
                            right = into
                            if (lang == lastOpen) down = FocusRequester.Cancel
                        },
                    enabled = taken == null,
                ) {
                    LanguageFlag(lang.key, 40.dp, if (taken != null) Modifier.alpha(TAKEN_ALPHA) else Modifier)
                    LanguageName(row, it.text, it.quiet, Modifier.weight(1f))
                    MqText("${lang.options.size}", role = TextRole.Callout, color = it.quiet, maxLines = 1)
                    TvCheck(on, it)
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
        // Left goes back to the row the options came from, the Spoken language row included; to Off with no options.
        val back =
            when {
                focusedKey == SPOKEN_ROW && spokenRow -> spokenFocus
                preview != null -> requester(preview.key)
                else -> off
            }
        if (preview != null) {
            Heading(preview.name)
            val target = preview.options.firstOrNull { it.selected } ?: preview.options.first()
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
                        { tab.pick(option.id, onPick) },
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
        }
        val foot = Modifier.padding(start = 16.dp, top = 12.dp)
        val left = Modifier.focusProperties { this.left = back }
        if (tab.second && second != null && (preview != null || footOnly)) {
            // Down chains Timing, Size, Layout, then Only when paused, whose Down stays (plans/dual-subtitles.md 5.3).
            Column(if (footOnly) Modifier.focusRequester(into) else Modifier) {
                second.timing?.let { TimingRow(it, second.onDelay, foot) }
                SizeRow(textSize, onSize, foot, first = left)
                LayoutRow(second.layout, second.onLayout, foot, first = left)
                PausedRow(
                    second.onPause,
                    second.onPauseChange,
                    // The row keeps its own inset, so its words line up with the rows over it.
                    Modifier.padding(top = 12.dp).focusProperties {
                        this.left = back
                        right = FocusRequester.Cancel
                        down = FocusRequester.Cancel
                    },
                )
            }
        } else if (preview != null) {
            if (timing != null) TimingRow(timing, onDelay, foot)
            SizeRow(
                textSize,
                onSize,
                foot,
                first =
                    Modifier.focusProperties {
                        this.left = back
                        down = FocusRequester.Cancel
                    },
            )
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
    /** False for a row of the other line's language: shown in quiet words, never focused or picked. */
    enabled: Boolean = true,
    content: @Composable RowScope.(RowInk) -> Unit,
) {
    val colors = Marquee.colors
    var focused by remember { mutableStateOf(false) }
    val lift by animateFloatAsState(if (focused) 1f else 0f, tween(Marquee.focusMs), label = "lift")
    val ink =
        when {
            focused -> colors.inverted().let { RowInk(it.text, it.dim, FontWeight.Medium) }
            !enabled -> RowInk(colors.dim, colors.dim, FontWeight.Medium)
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
            .focusClickable(onClick, Role.RadioButton, enabled = enabled)
            .clearAndSetSemantics {
                selected = on
                contentDescription = spoken
                if (!enabled) disabled()
            }.heightIn(min = 60.dp)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = if (top) Alignment.Top else Alignment.CenterVertically,
    ) { content(ink) }
}

/** The pickers, for tests. */
const val SUBTITLE_SHEET_TAG = "subtitle-sheet"
const val SUBTITLE_PICKER_TAG = "subtitle-picker"
