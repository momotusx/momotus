package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import app.marquee.core.model.SubtitleTrack
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

/** The subtitle picker's languages (plans/player-polish.md 2.4): grouping, order, the best option, search, counts. */
class SubtitleLanguagesTest {
    private fun track(
        url: String,
        lang: String,
        language: String,
        languageName: String,
        nativeName: String = languageName,
        name: String = "OpenSubtitles v3",
        forced: Boolean = false,
        fits: Boolean = false,
        release: String? = null,
    ) = SubtitleTrack(
        url = url,
        lang = lang,
        language = language,
        languageName = languageName,
        nativeName = nativeName,
        name = name,
        source = "OpenSubtitles v3",
        forced = forced,
        hearingImpaired = false,
        fits = fits,
        release = release,
    )

    // As the core sends them with English preferred: English, then the rest by name.
    private val core =
        listOf(
            track(
                "u/en1",
                "eng",
                "en",
                "English",
                name = "CAKES, WEB 1080p",
                release = "Slow.Horses.S06E02.1080p.WEB.H264-CAKES",
            ),
            track("u/en2", "eng", "en", "English", name = "FLUX, WEB-DL 1080p", fits = true),
            track("u/ar", "ara", "ar", "Arabic", "العربية"),
            track("u/pob", "pob", "pt-BR", "Portuguese (Brazil)", "Português (Brasil)", forced = true),
            track("u/tr", "tur", "tr", "Turkish", "Türkçe"),
        )

    /** Tracks inside the file, as Media3 names them: a forced English one, a plain one, and Japanese. */
    private val file =
        listOf(
            TrackChoice(
                "0:0",
                "English",
                "In the file · Forced",
                section = "English",
                language = "en",
                inFile = true,
                forced = true,
            ),
            TrackChoice("0:1", "English", "In the file", section = "English", language = "en", inFile = true),
            TrackChoice(
                "0:2",
                "Japanese",
                "In the file",
                section = "Japanese",
                language = "ja",
                nativeName = "日本語",
                inFile = true,
            ),
        )

    private fun rows(selected: String? = null) =
        subtitleRows(core, emptyMap(), emptySet(), file, "late:").map {
            if (it.id ==
                selected
            ) {
                it.copy(selected = true)
            } else {
                it
            }
        }

    @Test
    fun `languages in the core's order, the file's own first in each, a file-only language last`() {
        val languages = subtitleLanguages(rows())
        assertEquals(listOf("en", "ar", "pt-br", "tr", "ja"), languages.map { it.key })
        assertEquals(
            listOf("English", "Arabic", "Portuguese (Brazil)", "Turkish", "Japanese"),
            languages.map { it.name },
        )
        // The file's plain track, then its forced one, then the core's in its order.
        assertEquals(listOf("0:1", "0:0", "late:u/en1", "late:u/en2"), languages[0].options.map { it.id })
        // The own name only where it differs.
        assertEquals(listOf(null, "العربية", "Português (Brasil)", "Türkçe", "日本語"), languages.map { it.nativeName })
        assertEquals(listOf(4, 1, 1, 1, 1), languages.map { it.options.size })
    }

    @Test
    fun `picking a language turns on its best option, never a forced one while a plain one is there`() {
        val languages = subtitleLanguages(rows()).associateBy { it.key }
        assertEquals("0:1", languages.getValue("en").best.id, "the file's plain track: it always fits the file")
        assertEquals("late:u/ar", languages.getValue("ar").best.id)
        // A language with only a forced track still turns that one on.
        assertEquals("late:u/pob", languages.getValue("pt-br").best.id)
        // Without the file, the core's first track (its best match).
        assertEquals(
            "late:u/en1",
            subtitleLanguages(subtitleRows(core, emptyMap(), emptySet(), emptyList(), "late:"))[0].best.id,
        )
    }

    @Test
    fun `the pill counts the languages the picker shows, or names the chosen one`() {
        assertEquals("Off, 5 languages", subtitlesDetail(rows(), pending = false))
        assertEquals("Portuguese (Brazil)", subtitlesDetail(rows("late:u/pob"), pending = false))
        assertEquals("Japanese", subtitlesDetail(rows("0:2"), pending = false))
        val one = subtitleRows(core.take(2), emptyMap(), emptySet(), emptyList(), "late:")
        assertEquals("Off, 1 language", subtitlesDetail(one, pending = false))
        val none = subtitleRows(emptyList(), emptyMap(), emptySet(), emptyList(), "late:")
        assertEquals("None found", subtitlesDetail(none, pending = false))
        assertEquals("Searching", subtitlesDetail(none, pending = true))
    }

    @Test
    fun `search finds a language by its English name, its own name and its code`() {
        val languages = subtitleLanguages(rows())

        fun find(q: String) = filterLanguages(languages, q).map { it.key }
        assertEquals(languages.map { it.key }, find(" "), "a blank search shows them all")
        assertEquals(listOf("tr"), find("Tur"))
        assertEquals(listOf("tr"), find("turk"), "accents aside: Türkçe")
        assertEquals(listOf("pt-br"), find("brazil"), "a word inside the name")
        assertEquals(listOf("pt-br"), find("portug"))
        assertEquals(listOf("en"), find("eng"), "the three letters addons send")
        assertEquals(listOf("ja"), find("日本"))
        assertEquals(listOf("ar"), find("ar"))
        assertEquals(emptyList(), find("ish"), "the start of a word, not any part of it")
    }

    @Test
    fun `an option's line says where it is from, the release group match, then the release cut in the middle`() {
        val en = subtitleLanguages(rows())[0].options.associateBy { it.id }
        assertEquals("OpenSubtitles v3 · Slow…1080p.WEB.H264-CAKES", optionLine(en.getValue("late:u/en1"), 46))
        assertEquals("OpenSubtitles v3 · Same release group", optionLine(en.getValue("late:u/en2"), 46))
        assertEquals("In the file · Forced", optionLine(en.getValue("0:0"), 46))
        // Too little room left on the line: the release takes the whole measure on the next.
        val long = en.getValue("late:u/en1").copy(detail = "OpenSubtitles v3 · For the hard of hearing")
        assertEquals(
            "OpenSubtitles v3 · For the hard of hearing · Slow.Horses.S06E02.1080p.WEB.H264-CAKES",
            optionLine(long, 46),
        )
    }

    @Test
    fun `a release name is cut in its middle at a dot, keeping its group, as the frames cut it`() {
        // Each expected value is the frames' own `middle` (design/exploration/v3/src/v2/common.ts) on the same input.
        assertEquals("Slow…1080p.WEB.H264-CAKES", middleCut("Slow.Horses.S06E02.1080p.WEB.H264-CAKES", 27))
        assertEquals(
            "Slow…Atmos.H.264-FLUX",
            middleCut("Slow.Horses.S06E02.Daddy.Issues.1080p.ATVP.WEB-DL.DDP5.1.Atmos.H.264-FLUX", 24),
        )
        assertEquals(
            "Slow…x265-MeGusta[EZTVx.to]",
            middleCut("Slow.Horses.S06E02.720p.HEVC.x265-MeGusta[EZTVx.to]", 27),
        )
        assertEquals("slow…1080p.web.h264-cakes-hi", middleCut("slow.horses.s06e02.1080p.web.h264-cakes-hi", 34))
        assertEquals(
            "Slow.Horses.S06E02.1080p.WEB.H264-CAKES",
            middleCut("Slow.Horses.S06E02.1080p.WEB.H264-CAKES", 46),
        )
        assertEquals("abcdefghijklmnopqrs…", middleCut("abcdefghijklmnopqrstuvwxyz0123456789", 20))
    }

    @Test
    fun `a language takes its own flag, else its language's, else none`() {
        assertEquals(FLAGS["pt-br"], flagFor("pt-br"))
        assertNotNull(flagFor("pt-br"))
        assertEquals(FLAGS["zh"], flagFor("zh-hant"), "no flag for the script: the language's")
        assertNull(flagFor(""))
        assertNull(flagFor("zz"))
        // circle-flags' language set: about 200, none of them the question mark or a constructed set.
        assertEquals(true, FLAGS.size > 190 && "xx" !in FLAGS && "translingual" !in FLAGS)
    }

    @Test
    fun `the file's own plain track takes over from the start subtitle in its language`() {
        val all = rows()
        assertEquals("0:1", fileTakesOver("en", all)?.id)
        assertEquals("0:2", fileTakesOver("ja", all)?.id)
        assertNull(fileTakesOver("tr", all), "no track in the file in that language")
        assertNull(fileTakesOver("en", all.filterNot { it.id == "0:1" }), "a forced one never takes over")
        assertNull(
            fileTakesOver("", all + TrackChoice("0:3", "Subtitles", language = "", inFile = true)),
            "no language, no match",
        )
    }

    @Test
    fun `a track in the file goes under its language, with its own name, and says it is in the file`() {
        val forced =
            Format
                .Builder()
                .setLanguage("tur")
                .setSampleMimeType(MimeTypes.TEXT_VTT)
                .setSelectionFlags(C.SELECTION_FLAG_FORCED)
                .build()
        val row = textChoice("0:0", forced, selected = false)
        assertEquals(
            TrackChoice(
                "0:0",
                "Turkish",
                "In the file · WebVTT · Forced",
                section = "Turkish",
                language = "tr",
                nativeName = "Türkçe",
                inFile = true,
                forced = true,
            ),
            row,
        )
        val unnamed = textChoice("0:1", Format.Builder().setLabel("Signs").build(), selected = true)
        assertEquals(UNKNOWN_LANGUAGE, unnamed.section)
        assertEquals("", unnamed.language)
        assertEquals("Signs", unnamed.name)
    }
}
