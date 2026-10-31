package app.marquee.player

import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleProblem
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** The words of a subtitle note (plans/m13-turkcealtyazi.md 4.2, the same as the web's). */
class SubtitleNoteWordsTest {
    private fun note(problem: SubtitleProblem) = SubtitleNote("TurkceAltyazi.org", problem)

    @Test
    fun `each problem has its words, and one from a newer core the general line`() {
        assertEquals(
            listOf(
                "A subtitle source did not answer. Try its subtitles again in a moment.",
                "A subtitle source is turning requests away. Momotus will try again in a few minutes.",
                "A subtitle source changed its pages, so Momotus cannot read them. An update will fix this.",
                "This subtitle could not be read.",
                "Some subtitles did not load.",
            ),
            listOf(
                SubtitleProblem.Unreachable,
                SubtitleProblem.Blocked,
                SubtitleProblem.Changed,
                SubtitleProblem.TrackFailed,
                SubtitleProblem.Unknown,
            ).map { subtitleNoteWords(note(it)) },
        )
    }

    @Test
    fun `under the list the pending line comes first, then the notes in the core's order`() {
        val notes = listOf(note(SubtitleProblem.Blocked), SubtitleNote("SubSense", SubtitleProblem.Unreachable))
        assertEquals(
            "Still asking subtitle addons\n" +
                "A subtitle source is turning requests away. Momotus will try again in a few minutes.\n" +
                "A subtitle source did not answer. Try its subtitles again in a moment.",
            subtitlePanelNote(pending = true, notes = notes),
        )
        // Never a source's name (the owner's rule), and a line two sources share is said once.
        val alike = notes + SubtitleNote("OpenSubtitles v3", SubtitleProblem.Unreachable)
        val said = subtitlePanelNote(pending = false, notes = alike).orEmpty()
        assertEquals(
            listOf(
                "A subtitle source is turning requests away. Momotus will try again in a few minutes.",
                "A subtitle source did not answer. Try its subtitles again in a moment.",
            ),
            said.lines(),
        )
        listOf("TurkceAltyazi", "SubSense", "OpenSubtitles").forEach { assert(it !in said) { said } }
        assertEquals("Still asking subtitle addons", subtitlePanelNote(pending = true, notes = emptyList()))
        assertNull(subtitlePanelNote(pending = false, notes = emptyList()))
        // What just happened: the notes the last snapshot did not have.
        assertEquals(listOf(notes[1]), newSubtitleNotes(notes.take(1), notes))
        assertEquals(emptyList(), newSubtitleNotes(notes, notes))
    }
}
