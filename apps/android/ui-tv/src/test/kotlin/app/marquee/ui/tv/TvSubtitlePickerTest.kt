package app.marquee.ui.tv

import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.SEARCH_LANGUAGES
import app.marquee.player.SecondLine
import app.marquee.player.TrackChoice
import app.marquee.player.TvSubtitlePicker
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The TV's subtitle picker (plans/player-polish.md 2.4): two columns, D-pad focus moving right into the options. */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvSubtitlePickerTest {
    @get:Rule
    val compose = createComposeRule()

    /** English chosen (the core's CAKES track), the file's own English, Arabic and Turkish, as the controller lists them. */
    private val rows =
        listOf(
            TrackChoice("off", "Off"),
            TrackChoice(
                "late:en1",
                "CAKES, WEB 1080p",
                null,
                selected = true,
                section = "English",
                language = "en",
                release = "Slow.Horses.S06E02.1080p.WEB.H264-CAKES",
            ),
            TrackChoice(
                "late:en2",
                "FLUX, WEB-DL 1080p",
                null,
                section = "English",
                language = "en",
                fits = true,
            ),
            TrackChoice(
                "late:ar",
                "PSA, WEBRip 1080p",
                null,
                section = "Arabic",
                language = "ar",
                nativeName = "العربية",
            ),
            TrackChoice("late:tr", "Turkish 1", section = "Turkish", language = "tr", nativeName = "Türkçe"),
            TrackChoice("0:0", "English", "In the file", section = "English", language = "en", inFile = true),
        )

    private val picks = mutableListOf<String>()

    private fun open(timing: Long? = null) {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                var delay by remember { mutableLongStateOf(timing ?: 0L) }
                TvSubtitlePicker(
                    rows,
                    onPick = { picks += it },
                    timing = timing?.let { delay },
                    onDelay = { delay = it },
                )
            }
        }
        compose.waitForIdle()
    }

    private fun row(spoken: String) = compose.onNodeWithContentDescription(spoken)

    private fun SemanticsNodeInteraction.press(key: Key) = performKeyInput { pressKey(key) }

    @Test
    fun `focus opens on the chosen language, and the column at the right follows the focused one`() {
        open()
        row("English, 3 options").assertIsFocused()
        // The options of English: the file's own first, then the core's best match, said with its whole release text.
        compose.onNodeWithContentDescription("English, In the file").assertExists()
        compose
            .onNodeWithContentDescription(
                "CAKES, WEB 1080p, Slow.Horses.S06E02.1080p.WEB.H264-CAKES",
            ).assertExists()
        compose.onNodeWithContentDescription("FLUX, WEB-DL 1080p, Same release group").assertExists()
        row("English, 3 options").press(Key.DirectionDown)
        row("Arabic, العربية, 1 option").assertIsFocused()
        compose.onNodeWithText("Arabic").assertExists()
        compose.onAllNodesWithContentDescription("CAKES, WEB 1080p", substring = true).fetchSemanticsNodes().let {
            assertEquals(0, it.size, "English's options gave way to Arabic's")
        }
    }

    @Test
    fun `Right moves into the options, on the chosen one, and Left comes back to its language`() {
        open()
        row("English, 3 options").press(Key.DirectionRight)
        row("CAKES, WEB 1080p, Slow.Horses.S06E02.1080p.WEB.H264-CAKES").assertIsFocused()
        row("CAKES, WEB 1080p, Slow.Horses.S06E02.1080p.WEB.H264-CAKES").press(Key.DirectionLeft)
        row("English, 3 options").assertIsFocused()
        // A language with nothing chosen opens on its first option.
        row("English, 3 options").press(Key.DirectionDown)
        row("Arabic, العربية, 1 option").press(Key.DirectionRight)
        row("PSA, WEBRip 1080p").assertIsFocused()
    }

    @Test
    fun `Left from a language and from Off stays in the picker, with a control at its left`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Box(Modifier.fillMaxSize()) {
                    // Something focusable left of the picker and level with its rows, as a player's controls may be.
                    Box(
                        Modifier
                            .padding(top = 120.dp)
                            .size(200.dp, 600.dp)
                            .focusable()
                            .semantics { contentDescription = "Behind" },
                    )
                    TvSubtitlePicker(rows, {}, Modifier.align(Alignment.TopEnd).padding(end = 80.dp, top = 96.dp))
                }
            }
        }
        compose.waitForIdle()
        row("English, 3 options").assertIsFocused().press(Key.DirectionLeft)
        row("English, 3 options").assertIsFocused()
        row("English, 3 options").press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.DirectionLeft)
        row("Off").assertIsFocused()
    }

    @Test
    fun `selecting a language turns on its best option at once, an option itself, and Off`() {
        open()
        row("English, 3 options").press(Key.Enter)
        row("English, 3 options").press(Key.DirectionDown)
        row("Arabic, العربية, 1 option").press(Key.DirectionDown)
        row("Turkish, Türkçe, 1 option").press(Key.Enter)
        row("Turkish, Türkçe, 1 option").press(Key.DirectionRight)
        row("Turkish 1").press(Key.Enter)
        row("Turkish 1").press(Key.DirectionLeft)
        // Up the list to Off.
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        row("Off").assertIsFocused().press(Key.Enter)
        compose.runOnIdle { assertEquals(listOf("0:0", "late:tr", "late:tr", "off"), picks) }
    }

    @Test
    fun `the search field over Off finds a language by its own name`() {
        open()
        row("English, 3 options").press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.DirectionUp)
        // As the TV's keyboard fills it.
        compose
            .onNode(
                isFocused() and hasContentDescription(SEARCH_LANGUAGES),
            ).performSemanticsAction(SemanticsActions.SetText) {
                it(AnnotatedString("türk"))
            }
        compose.waitForIdle()
        row("Turkish, Türkçe, 1 option").assertExists()
        assertEquals(0, compose.onAllNodesWithContentDescription("Arabic", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun `Down from the last option reaches the Size row, which steps the profile's size`() {
        val sizes = mutableListOf<SubtitleSize>()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvSubtitlePicker(rows, onPick = {}, textSize = SubtitleSize.Medium, onSize = {
                    sizes +=
                        it
                })
            }
        }
        compose.waitForIdle()
        row("English, 3 options").press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused() and hasText("Smaller")).assertExists()
        compose.onNode(isFocused()).press(Key.DirectionRight)
        compose.onNode(isFocused() and hasText("Larger")).assertExists().press(Key.Enter)
        compose.onNodeWithText("100%").assertExists()
        compose.runOnIdle { assertEquals(listOf(SubtitleSize.Large), sizes) }
    }

    @Test
    fun `Down from the last option reaches the delay, where Left and Right step it, held faster, and select resets`() {
        open(timing = 0L)
        row("English, 3 options").press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        val delay = compose.onNode(isFocused() and hasContentDescription("Timing"))
        delay.assertExists()
        compose.onNodeWithText("0.0 s").assertExists()
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionRight) }
        compose.onNodeWithText("+0.3 s").assertExists()
        compose.onNode(isFocused()).press(Key.DirectionLeft)
        compose.onNodeWithText("+0.2 s").assertExists()
        // The focus stayed on the delay all along; select puts it back to 0.
        compose.onNode(isFocused() and hasContentDescription("Timing")).press(Key.Enter)
        compose.onNodeWithText("0.0 s").assertExists()
        // Right held for a second: 0.3 s a tick from 350 ms, every 100 ms (7 ticks), whatever the system's own key repeats.
        compose.mainClock.autoAdvance = false
        compose.onNode(isFocused()).performKeyInput { keyDown(Key.DirectionRight) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNode(isFocused()).performKeyInput { keyUp(Key.DirectionRight) }
        compose.mainClock.advanceTimeBy(500)
        compose.mainClock.autoAdvance = true
        compose.onNodeWithText("+2.1 s").assertExists()
        // Down from the delay reaches Reset, shown while it is not 0.
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused() and hasText("Reset")).assertExists().press(Key.Enter)
        compose.onNodeWithText("0.0 s").assertExists()
        compose.onNodeWithText("Reset").assertDoesNotExist()
        compose.onNode(isFocused() and hasContentDescription("Timing")).assertExists()
    }

    // The second line (plans/dual-subtitles.md 5.3).

    /** Turkish on the first line, English on the second (the core's tracks only: no track in the file). */
    private val secondRows =
        listOf(
            TrackChoice("off", "Off"),
            TrackChoice("late:en1", "CAKES, WEB 1080p", null, selected = true, section = "English", language = "en"),
            TrackChoice("late:en2", "FLUX, WEB-DL 1080p", null, section = "English", language = "en"),
            TrackChoice(
                "late:ar",
                "PSA, WEBRip 1080p",
                null,
                section = "Arabic",
                language = "ar",
                nativeName = "العربية",
            ),
            TrackChoice("late:tr", "Turkish 1", section = "Turkish", language = "tr", nativeName = "Türkçe"),
        )

    /** The same rows with Turkish chosen on the first line. */
    private val firstRows = rows.map { it.copy(selected = it.id == "late:tr") }

    private val secondPicks = mutableListOf<String>()
    private val pauses = mutableListOf<Boolean>()
    private val layouts = mutableListOf<SubtitleLayout>()
    private var spokenPressed = 0

    private fun secondLine(
        spokenOn: Boolean = false,
        rows: List<TrackChoice> = secondRows,
        spoken: String? = "en",
        timing: Long? = 0L,
    ) = SecondLine(
        rows = rows,
        firstKey = "tr",
        secondKey = "en",
        spoken = spoken,
        spokenOn = spokenOn,
        pending = false,
        timing = timing,
        layout = SubtitleLayout.Stacked,
        onPause = false,
        onPick = { secondPicks += it },
        onSpoken = { spokenPressed++ },
        onDelay = {},
        onLayout = { layouts += it },
        onPauseChange = { pauses += it },
    )

    private fun openTwo(second: SecondLine? = secondLine()) {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                var tab by remember { mutableStateOf(false) }
                TvSubtitlePicker(
                    firstRows,
                    onPick = { picks += it },
                    second = second,
                    secondTab = tab,
                    onTab = { tab = it },
                )
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun `Up from search reaches the switch, Left and Right stop at its ends, OK lands on the tab's chosen row`() {
        openTwo()
        row("Turkish, Türkçe, 1 option").assertIsFocused()
        // English is the second line's: it reads so here, and the remote passes it by.
        row("English, On the second line, 3 options").assertIsNotEnabled()
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        row("Arabic, العربية, 1 option").assertIsFocused().press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.DirectionUp)
        compose.onNode(isFocused() and hasContentDescription(SEARCH_LANGUAGES)).assertExists().press(Key.DirectionUp)
        compose.onNode(isFocused() and hasText("First line")).assertExists().press(Key.DirectionLeft)
        compose.onNode(isFocused() and hasText("First line")).assertExists().press(Key.DirectionUp)
        compose.onNode(isFocused() and hasText("First line")).assertExists().press(Key.DirectionRight)
        compose.onNode(isFocused() and hasText("Second line")).assertExists().press(Key.DirectionRight)
        compose.onNode(isFocused() and hasText("Second line")).assertExists().press(Key.Enter)
        // The Second line tab: its chosen row takes the focus; the first line's Turkish is taken.
        compose.waitForIdle()
        row("English, 2 options").assertIsFocused()
        row("Turkish, On the first line, 1 option").assertIsNotEnabled()
        row("Spoken language, English").assertExists()
        // Back up to the switch and over to First line: its chosen row again.
        row("English, 2 options").press(Key.DirectionUp)
        row("Spoken language, English").assertIsFocused().press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.DirectionUp)
        compose.onNode(isFocused()).press(Key.DirectionUp)
        compose.onNode(isFocused() and hasText("Second line")).assertExists().press(Key.DirectionLeft)
        compose.onNode(isFocused() and hasText("First line")).press(Key.Enter)
        compose.waitForIdle()
        row("Turkish, Türkçe, 1 option").assertIsFocused()
        compose.runOnIdle { assertEquals(emptyList(), picks + secondPicks) }
    }

    @Test
    fun `the Second line tab picks into the second line, its Spoken language row included`() {
        openTwo()
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        compose.onNode(isFocused() and hasText("First line")).press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.Enter)
        compose.waitForIdle()
        row("English, 2 options").assertIsFocused().press(Key.DirectionDown)
        row("Arabic, العربية, 1 option").assertIsFocused().press(Key.Enter)
        // Down from Arabic passes the taken Turkish and stays: Arabic is the last that can be picked.
        row("Arabic, العربية, 1 option").press(Key.DirectionDown)
        row("Arabic, العربية, 1 option").assertIsFocused()
        repeat(2) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        row("Spoken language, English").assertIsFocused().press(Key.Enter)
        compose.onNode(isFocused()).press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.Enter)
        compose.runOnIdle {
            assertEquals(listOf("late:ar", "off"), secondPicks)
            assertEquals(1, spokenPressed)
            assertEquals(emptyList(), picks, "a Second line pick went to the first line")
        }
    }

    @Test
    fun `the second line's options end with Timing, Size, Layout and Only when paused, whose Down stays`() {
        openTwo()
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        compose.onNode(isFocused() and hasText("First line")).press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.Enter)
        compose.waitForIdle()
        row("English, 2 options").press(Key.DirectionRight)
        row("CAKES, WEB 1080p").assertIsFocused().press(Key.DirectionDown)
        row("FLUX, WEB-DL 1080p").assertIsFocused().press(Key.DirectionDown)
        compose.onNode(isFocused() and hasContentDescription("Timing")).assertExists().press(Key.DirectionDown)
        compose
            .onNode(
                isFocused() and (hasText("Smaller") or hasText("Larger")),
            ).assertExists()
            .press(Key.DirectionDown)
        compose
            .onNode(
                isFocused() and (hasContentDescription("Previous layout") or hasContentDescription("Next layout")),
            ).assertExists()
            .press(Key.DirectionRight)
        compose.onNodeWithText("Stacked").assertExists()
        compose.onNode(isFocused() and hasContentDescription("Next layout")).assertExists().press(Key.Enter)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused() and hasText("Only when paused")).assertExists().press(Key.DirectionDown)
        compose.onNode(isFocused() and hasText("Only when paused")).assertExists().press(Key.Enter)
        compose.runOnIdle {
            assertEquals(listOf(true), pauses)
            assertEquals(listOf(SubtitleLayout.Split), layouts)
        }
    }

    /**
     * The Second line tab where no language can be picked (Turkish, the only one, is the first line's), the audio in
     * [spoken]: Right from the row focus opens on reaches the foot, which steps and switches, and Left comes back.
     */
    private fun footWithNothingToPick(
        spoken: String?,
        opensOn: String,
    ) {
        val onlyFirst = secondRows.filter { it.id == "off" || it.language == "tr" }
        // Nothing on the second line, so no Timing: the foot opens on Size.
        openTwo(secondLine(spokenOn = true, rows = onlyFirst, spoken = spoken, timing = null))
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        compose.onNode(isFocused() and hasText("First line")).press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.Enter)
        compose.waitForIdle()
        row("Turkish, On the first line, 1 option").assertIsNotEnabled()
        // The taken language never shows its options here, even as the Spoken language.
        compose.onNodeWithText("Turkish 1").assertDoesNotExist()
        row(opensOn).assertIsFocused().press(Key.DirectionRight)
        compose.onNode(isFocused() and hasText("Smaller")).assertExists().press(Key.DirectionDown)
        compose
            .onNode(
                isFocused() and hasContentDescription("Previous layout"),
            ).assertExists()
            .press(Key.DirectionRight)
        compose.onNode(isFocused() and hasContentDescription("Next layout")).press(Key.Enter)
        compose.onNode(isFocused()).press(Key.DirectionDown)
        compose.onNode(isFocused() and hasText("Only when paused")).assertExists().press(Key.Enter)
        compose.onNode(isFocused()).press(Key.DirectionLeft)
        row(opensOn).assertIsFocused()
        compose.runOnIdle {
            assertEquals(listOf(true), pauses)
            assertEquals(listOf(SubtitleLayout.Split), layouts)
        }
    }

    @Test
    fun `with no language to pick on the Second line tab, Right from Off reaches Layout and Only when paused`() =
        footWithNothingToPick(spoken = "tr", opensOn = "Off")

    @Test
    fun `with no language to pick and the audio's not known, Right from Spoken language reaches the foot`() =
        footWithNothingToPick(spoken = null, opensOn = "Spoken language, not known for this video")

    @Test
    fun `with a search typed, Left from the lone foot comes back to Off, never to the hidden Spoken language row`() {
        val onlyFirst = secondRows.filter { it.id == "off" || it.language == "tr" }
        openTwo(secondLine(spokenOn = true, rows = onlyFirst, spoken = null, timing = null))
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        compose.onNode(isFocused() and hasText("First line")).press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.Enter)
        compose.waitForIdle()
        row("Spoken language, not known for this video").assertIsFocused().press(Key.DirectionUp)
        row("Off").assertIsFocused().press(Key.DirectionUp)
        compose
            .onNode(isFocused() and hasContentDescription(SEARCH_LANGUAGES))
            .performSemanticsAction(SemanticsActions.SetText) { it(AnnotatedString("t")) }
        compose.waitForIdle()
        row("Spoken language, not known for this video").assertDoesNotExist()
        compose.onNode(hasContentDescription(SEARCH_LANGUAGES)).press(Key.DirectionDown)
        row("Off").assertIsFocused().press(Key.DirectionRight)
        compose.onNode(isFocused() and hasText("Smaller")).assertExists().press(Key.DirectionLeft)
        row("Off").assertIsFocused()
    }

    @Test
    fun `with languages to pick, Right from a Spoken language not known stays, the foot under a language's options`() {
        openTwo(secondLine(spokenOn = true, spoken = null, timing = null))
        row("Turkish, Türkçe, 1 option").press(Key.DirectionUp)
        repeat(3) { compose.onNode(isFocused()).press(Key.DirectionUp) }
        compose.onNode(isFocused() and hasText("First line")).press(Key.DirectionRight)
        compose.onNode(isFocused()).press(Key.Enter)
        compose.waitForIdle()
        row("Spoken language, not known for this video").assertIsFocused().press(Key.DirectionRight)
        row("Spoken language, not known for this video").assertIsFocused()
        compose.onNodeWithText("Only when paused").assertDoesNotExist()
    }

    @Test
    fun `no switch while no second line can be on, and the first line's rows are today's`() {
        openTwo(second = null)
        compose.onNodeWithText("First line").assertDoesNotExist()
        compose.onNodeWithText("Second line").assertDoesNotExist()
        row("English, 3 options").assertIsEnabled()
    }
}
