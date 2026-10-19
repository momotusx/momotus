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
                "TurkceAltyazi.org did not answer. Try its subtitles again in a moment.",
                "TurkceAltyazi.org is turning requests away. Marquee will try again in a few minutes.",
                "TurkceAltyazi.org changed its pages, so Marquee cannot read them. An update will fix this.",
                "A subtitle file from TurkceAltyazi.org could not be read. Pick another one.",
                "Subtitles from TurkceAltyazi.org did not load.",
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
                "TurkceAltyazi.org is turning requests away. Marquee will try again in a few minutes.\n" +
                "SubSense did not answer. Try its subtitles again in a moment.",
            subtitlePanelNote(pending = true, notes = notes),
        )
        assertEquals("Still asking subtitle addons", subtitlePanelNote(pending = true, notes = emptyList()))
        assertNull(subtitlePanelNote(pending = false, notes = emptyList()))
        // What just happened: the notes the last snapshot did not have.
        assertEquals(listOf(notes[1]), newSubtitleNotes(notes.take(1), notes))
        assertEquals(emptyList(), newSubtitleNotes(notes, notes))
    }
}
