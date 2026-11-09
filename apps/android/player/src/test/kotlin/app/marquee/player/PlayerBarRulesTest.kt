package app.marquee.player

import androidx.compose.ui.unit.dp
import java.io.File
import kotlin.math.pow
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The bar's pure rules (plans/player-bar.md): the phone's fit (D17), a TV side's (D16), the strip's chips (D5), the scrim. */
class PlayerBarRulesTest {
    // A series on a phone: Audio, Speed, Versions; Subtitles, Episodes, Next; the clocks; the shift's two widths.
    private val groups = 48.dp * 6 + 6.dp * 5 + 8.dp * 2
    private val track = 46.dp * 2 + 8.dp * 2
    private val words = 200.dp
    private val chevrons = 172.dp

    @Test
    fun `a phone keeps one row with words while the scrubber keeps 120 dp, then chevrons, then two rows`() {
        val one = groups + track + words + MIN_SCRUB
        assertEquals(PhoneBarFit(1, shiftWords = true), phoneBarFit(one, groups, track, words, chevrons))
        assertEquals(PhoneBarFit(1, shiftWords = false), phoneBarFit(one - 1.dp, groups, track, words, chevrons))
        val chevronRow = groups + track + chevrons + MIN_SCRUB
        assertEquals(PhoneBarFit(1, shiftWords = false), phoneBarFit(chevronRow, groups, track, words, chevrons))
        // Too narrow even with chevrons: two rows, the words back when the control row has room for them.
        assertEquals(PhoneBarFit(2, shiftWords = true), phoneBarFit(chevronRow - 1.dp, groups, track, words, chevrons))
        val narrow = groups + words - 1.dp
        assertEquals(PhoneBarFit(2, shiftWords = false), phoneBarFit(narrow, groups, track, words, chevrons))
    }

    @Test
    fun `a TV side drops its words first, then the shift's, and never decides from the shift's presence`() {
        assertEquals(SideFit(true, true), sideFit(700.dp, words = 400.dp, icons = 200.dp, shiftWords = 300.dp))
        assertEquals(SideFit(false, true), sideFit(699.dp, words = 400.dp, icons = 200.dp, shiftWords = 300.dp))
        assertEquals(
            SideFit(false, false),
            sideFit(499.dp, words = 400.dp, icons = 200.dp, shiftWords = 300.dp),
        )
        // The left side has no shift: its words alone decide.
        assertEquals(SideFit(true, true), sideFit(400.dp, words = 400.dp, icons = 200.dp))
    }

    @Test
    fun `the strip's chips are the language on now, then the picker's order, at most the cap`() {
        fun row(
            id: String,
            lang: String,
            on: Boolean = false,
        ) = TrackChoice(id, lang, section = lang, language = lang, selected = on)
        val rows =
            listOf(
                TrackChoice(TRACK_OFF, "Off"),
                row("a", "tr"),
                row("b", "en"),
                row("c", "de"),
                row("d", "fr", on = true),
                row("e", "es"),
            )
        assertEquals(listOf("fr", "tr", "en"), stripLanguages(rows, 3).map { it.key })
        assertEquals(listOf("fr", "tr", "en", "de", "es"), stripLanguages(rows, 5).map { it.key })
        assertEquals(listOf("tr", "en", "de"), stripLanguages(rows.map { it.copy(selected = false) }, 3).map { it.key })
        assertEquals(emptyList(), stripLanguages(listOf(TrackChoice(TRACK_OFF, "Off")), 3))
    }

    @Test
    fun `the foot scrim keeps an open strip's chip words at 4_5 to 1 over a white picture`() {
        // A bar of 60 dp with a strip and Style row of 120 over it: the scrim reaches above both.
        val cover = 60.dp + 8.dp + 120.dp
        val height = footScrim(cover, 160.dp)
        assertTrue(height >= cover, "the scrim is under the strip's top: $height")
        val alpha = scrimAlpha(top = false, peak = 0.85f, at = 1f - cover / height)
        // White picture under black at that alpha, then the chip's white at 16 percent, blended in sRGB.
        val ground = (1f - alpha) * 255f
        val chip = ground * 0.84f + 255f * 0.16f
        val contrast = 1.05f / (luminance(chip) + 0.05f)
        assertTrue(contrast >= 4.5f, "white chip words read at $contrast:1 (scrim $alpha at the strip's top)")
        // A short bar keeps the least height.
        assertEquals(160.dp, footScrim(60.dp, 160.dp))
    }

    @Test
    fun `a remote enters the strip on the checked chip, else on Off, even for a track with no language`() {
        val english = TrackChoice("e", "English", section = "English", language = "en")
        val off = listOf(TrackChoice(TRACK_OFF, "Off", selected = true), english)
        assertTrue(offIsEntry(off, stripLanguages(off, 3)))
        val on = listOf(TrackChoice(TRACK_OFF, "Off"), english.copy(selected = true))
        assertTrue(!offIsEntry(on, stripLanguages(on, 3)))
        // Captions in the video that name no language: no chip holds them, so Off is the way in.
        val captions = listOf(TrackChoice(TRACK_OFF, "Off"), english, TrackChoice("0:0", "CC", selected = true))
        assertTrue(offIsEntry(captions, stripLanguages(captions, 3)))
    }

    @Test
    fun `a TV side counts the widest value a pill can show and its busy mark`() {
        // Value 300 wide on a TV pill, drawn up to 300 (twice the phone's cap), plus the spinner and its gap.
        val widths =
            BarWidths({ if (it == "long") 300.dp else 50.dp }, 72.dp, 36.dp, 24.dp, 14.dp, 200.dp, 15.dp, 300.dp, 20.dp)
        val pill = 24.dp * 2 + 36.dp + 14.dp + 50.dp
        assertEquals(pill + 14.dp + 300.dp, widths.pill("Subtitles", listOf("short", "long")))
        assertEquals(pill + 14.dp + 300.dp + 14.dp + 20.dp, widths.pill("Subtitles", listOf("long"), busy = true))
        // A series' right side as icons with chevrons fits the 1920 canvas' column (about 700) with room to spare.
        val icons = 72.dp * 3 + 15.dp * 3
        assertEquals(SideFit(false, false), sideFit(700.dp, icons + 2_000.dp, icons, 1_000.dp))
        assertTrue(icons + widths.shiftChevrons <= 700.dp)
    }

    @Test
    fun `positions step 5 to 25, 8 reading Default`() {
        assertEquals(listOf(5, 8, 12, 16, 20, 25), POSITIONS)
        assertEquals("Default", positionWords(8))
        assertEquals("12%", positionWords(12))
    }

    /** The relative luminance of an sRGB grey [value] 0 to 255. */
    @Test
    fun `the strips take Off, Still searching and Choose a version from the shared words`() {
        // Review 11 of bar-android: the words are player.bar's (StreamChoiceCopyTest holds them to the file), never
        // literals here, so the web and Apple cannot drift.
        val strip =
            File("src/main/kotlin/app/marquee/player/PlayerStrip.kt")
                .readLines()
                .filterNot { it.trimStart().startsWith("*") || it.trimStart().startsWith("/") }
                .joinToString("\n")
        for (literal in listOf("\"Off\"", "\"Still searching\"", "\"Choose a version\"")) {
            assertTrue(literal !in strip, "$literal is a literal in PlayerStrip.kt")
        }
        for (word in listOf("Bar.OFF", "Bar.STILL_SEARCHING", "Bar.CHOOSE_VERSION")) assertTrue(word in strip, word)
    }

    private fun luminance(value: Float): Float {
        val c = value / 255f
        return if (c <= 0.04045f) c / 12.92f else ((c + 0.055f) / 1.055f).pow(2.4f)
    }
}
