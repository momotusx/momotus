package app.marquee.player

import app.marquee.core.client.CoreJson
import app.marquee.core.model.SubtitleTrack
import kotlinx.serialization.decodeFromString
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The subtitles menu as the core orders and names the tracks (plans/m13-subtitle-languages.md 2 and 4). */
class SubtitleRowsTest {
    private fun track(
        url: String,
        lang: String,
        language: String,
        languageName: String,
        name: String,
        source: String = "OpenSubtitles v3",
        forced: Boolean = false,
        hearingImpaired: Boolean = false,
    ) = SubtitleTrack(
        url = url,
        lang = lang,
        language = language,
        languageName = languageName,
        name = name,
        source = source,
        forced = forced,
        hearingImpaired = hearingImpaired,
    )

    // As the core sends them with Turkish preferred: Turkish first, then the rest by name.
    private val core =
        listOf(
            track("u/tur", "tur", "tr", "Turkish", "Breaking.Bad.S01E01.720p", source = "SubSense"),
            track("u/eng1", "eng", "en", "English", "OpenSubtitles v3 1"),
            track("u/eng2", "eng", "en", "English", "OpenSubtitles v3 2", hearingImpaired = true),
            track("u/pob", "pob", "pt-BR", "Portuguese (Brazil)", "OpenSubtitles v3", forced = true),
        )

    @Test
    fun `Off, then the core's tracks in its order with their languages, then the file's tracks`() {
        val loaded = mapOf("u/eng2" to TrackChoice("ext:1", "", selected = true))
        val file =
            listOf(
                TrackChoice("0:0", "Spanish", "In the file · SRT", section = "Spanish", language = "es", inFile = true),
            )
        val rows = subtitleRows(core, loaded, emptySet(), file, "late:")
        assertEquals(
            listOf(
                TrackChoice("off", "Off", selected = false),
                TrackChoice(
                    "late:u/tur",
                    "Breaking.Bad.S01E01.720p",
                    // No source: no addon is named where a viewer reads it.
                    null,
                    section = "Turkish",
                    language = "tr",
                ),
                TrackChoice("late:u/eng1", "OpenSubtitles v3 1", section = "English", language = "en"),
                // Loaded: the player's id and state.
                TrackChoice("ext:1", "OpenSubtitles v3 2", "For the hard of hearing", true, "English", language = "en"),
                TrackChoice(
                    "late:u/pob",
                    "OpenSubtitles v3",
                    "Forced",
                    section = "Portuguese (Brazil)",
                    language = "pt-br",
                    forced = true,
                ),
                file[0],
            ),
            rows,
        )
        // The Subtitles button names the chosen track's language.
        assertEquals("English", subtitlesDetail(rows, pending = false))
    }

    @Test
    fun `a row carries the core's hash match and the frame rate the track was made for`() {
        val hashed = core[1].copy(hashMatch = true, madeForFps = 25f)
        val row = subtitleRows(listOf(hashed), emptyMap(), emptySet(), emptyList(), "late:")[1]
        assertEquals(true, row.hashMatch)
        assertEquals(25f, row.madeForFps)
    }

    @Test
    fun `nothing chosen leaves Off on, and a file that did not load is left out`() {
        val rows = subtitleRows(core, emptyMap(), setOf("u/tur"), emptyList(), "late:")
        assertEquals(true, rows.first().selected)
        assertEquals(listOf("late:u/eng1", "late:u/eng2", "late:u/pob"), rows.drop(1).map { it.id })
        assertEquals("Off, 2 languages", subtitlesDetail(rows, pending = false))
        // A track in the file says its language on the button.
        val file =
            subtitleRows(
                emptyList(),
                emptyMap(),
                emptySet(),
                listOf(
                    TrackChoice("0:0", "Spanish", selected = true, section = "Spanish", language = "es", inFile = true),
                ),
                "late:",
            )
        assertEquals("Spanish", subtitlesDetail(file, pending = false))
    }

    @Test
    fun `a core without the new fields still names and heads its tracks`() {
        val old =
            SubtitleTrack(
                url = "u/x",
                lang = "ger",
                label = "German",
                source = "SubSense",
                forced = false,
                hearingImpaired = false,
            )
        val row = subtitleRows(listOf(old), emptyMap(), emptySet(), emptyList(), "late:")[1]
        assertEquals("German", row.name)
        assertEquals("German", row.section)
        assertEquals("de", row.language)
        assertNull(row.detail, "no source under the name")
        // With no label either, the track is named by its language, never by its addon.
        val bare = subtitleRows(listOf(old.copy(label = null)), emptyMap(), emptySet(), emptyList(), "late:")[1]
        assertEquals("German", bare.name)
        assertNull(bare.detail)
        assertNull(subtitleRows(emptyList(), emptyMap(), emptySet(), emptyList(), "late:").first().section)
    }

    @Test
    fun `the start subtitle turns on once, unless the viewer chose or a carried pick is on`() {
        assertEquals("u/tur", startSubtitle("u/tur", chose = false, picked = false, started = null))
        assertNull(startSubtitle(null, chose = false, picked = false, started = null))
        assertNull(startSubtitle("u/tur", chose = true, picked = false, started = null), "the viewer chose (or Off)")
        assertNull(startSubtitle("u/tur", chose = false, picked = true, started = null), "a carried pick is on")
        assertNull(startSubtitle("u/tur", chose = false, picked = false, started = "u/tur"), "once")
        assertEquals("u/eng", startSubtitle("u/eng", chose = false, picked = false, started = "u/tur"))
    }

    /**
     * What the core sends for Sintel (apps/shared/fixtures/playback-subtitles-sintel.json, kept true by the core's
     * own test): OpenSubtitles v3's real answer and SubSense's labels ("OpenSubtitles · [SRT] · sintel.he.srt").
     * Every option reads as a viewer reads it, sintel.he under Hebrew.
     */
    @Test
    fun `Sintel's real list reads clean, with no addon, format or extension`() {
        val text = File("../../shared/fixtures/playback-subtitles-sintel.json").readText()
        val core = CoreJson.decodeFromString<List<SubtitleTrack>>(text)
        val rows = subtitleRows(core, emptyMap(), emptySet(), emptyList(), "late:").drop(1)
        val raw = Regex("opensubtitles|subsense|\\[|\\.srt|·", RegexOption.IGNORE_CASE)
        for (row in rows) {
            val line = listOfNotNull(row.name, row.detail, row.release).joinToString(" | ")
            assertFalse(raw.containsMatchIn(line), line)
            assertNotEquals(row.name, row.release, line)
        }
        val languages = subtitleLanguages(rows).associateBy { it.name }
        assertEquals((1..6).map { "English $it" }, languages.getValue("English").options.map { it.name })
        assertTrue("Hebrew 4" in languages.getValue("Hebrew").options.map { it.name })
        // An older core sends no name: the track reads as its language, never the addon's label.
        val old =
            SubtitleTrack(
                url = "u/old",
                lang = "heb",
                label = "OpenSubtitles · [SRT] · sintel.he.srt",
                source = "SubSense",
                forced = false,
                hearingImpaired = false,
            )
        assertEquals("Hebrew", subtitleRows(listOf(old), emptyMap(), emptySet(), emptyList(), "late:")[1].name)
    }
}
