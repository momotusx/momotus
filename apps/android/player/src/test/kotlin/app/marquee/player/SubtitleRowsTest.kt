package app.marquee.player

import app.marquee.core.model.SubtitleTrack
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

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
    fun `Off, then the core's languages in its order under their names, then the file's tracks`() {
        val loaded = mapOf("u/eng2" to TrackChoice("ext:1", "", selected = true))
        val file = listOf(TrackChoice("0:0", "Spanish", "SRT"))
        val rows = subtitleRows(core, loaded, emptySet(), file, "late:")
        assertEquals(
            listOf(
                TrackChoice("off", "Off", selected = false),
                TrackChoice(
                    "late:u/tur",
                    "Breaking.Bad.S01E01.720p",
                    "SubSense, loads when chosen",
                    section = "Turkish",
                ),
                TrackChoice("late:u/eng1", "OpenSubtitles v3 1", "loads when chosen", section = "English"),
                // Loaded: the player's id and state; the name already says its source.
                TrackChoice("ext:1", "OpenSubtitles v3 2", "For the hard of hearing", true, "English"),
                TrackChoice(
                    "late:u/pob",
                    "OpenSubtitles v3",
                    "Forced, loads when chosen",
                    section = "Portuguese (Brazil)",
                ),
                TrackChoice("0:0", "Spanish", "SRT", section = "In the file"),
            ),
            rows,
        )
        // The Subtitles button names the chosen track's language.
        assertEquals("English", subtitlesDetail(rows, pending = false))
    }

    @Test
    fun `nothing chosen leaves Off on, and a file that did not load is left out`() {
        val rows = subtitleRows(core, emptyMap(), setOf("u/tur"), emptyList(), "late:")
        assertEquals(true, rows.first().selected)
        assertEquals(listOf("late:u/eng1", "late:u/eng2", "late:u/pob"), rows.drop(1).map { it.id })
        assertEquals("Off, 3 found", subtitlesDetail(rows, pending = false))
        // A track in the file says its own name on the button.
        val file =
            subtitleRows(
                emptyList(),
                emptyMap(),
                emptySet(),
                listOf(TrackChoice("0:0", "Spanish", selected = true)),
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
        assertEquals("SubSense, loads when chosen", row.detail)
        assertNull(subtitleRows(emptyList(), emptyMap(), emptySet(), emptyList(), "late:").first().section)
    }
}
