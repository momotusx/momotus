package app.marquee.core

import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails
import kotlin.test.Test
import kotlin.test.assertEquals

class TitleCopyTest {
    @Test
    fun `the play label adds where playback resumes`() {
        assertEquals("Play", TitleCopy.playLabel(PlayTarget("tt1", "Play", null)))
        assertEquals("Play S1:E1", TitleCopy.playLabel(PlayTarget("tt1:1:1", "Play S1:E1", 0)))
        assertEquals("Resume S1:E2 from 41:12", TitleCopy.playLabel(PlayTarget("tt1:1:2", "Resume S1:E2", 2_472_000)))
    }

    private fun details(
        releaseInfo: String? = "2010",
        runtime: String? = "52 min",
        rating: String? = "7.8",
        genres: List<String> = listOf("Animation", "Short", "Fantasy", "Extra"),
    ) = TitleDetails(
        id = "tt1",
        kind = "movie",
        name = "Sintel",
        releaseInfo = releaseInfo,
        runtime = runtime,
        rating = rating,
        genres = genres,
        cast = emptyList(),
        directors = emptyList(),
        source = "Cinemeta",
        trailers = emptyList(),
    )

    @Test
    fun `the meta line keeps what the metadata gives and skips the rest`() {
        assertEquals("2010 · 52 min · Rated 7.8 · Animation, Short, Fantasy", TitleCopy.meta(details()))
        assertEquals("2010", TitleCopy.meta(details(runtime = null, rating = null, genres = emptyList())))
        assertEquals(
            "",
            TitleCopy.meta(details(releaseInfo = null, runtime = null, rating = null, genres = emptyList())),
        )
    }

    @Test
    fun `an episode reads as watched state, number and title`() {
        val episode = Episode("tt1:2:1", 2, 1, "The North Remembers", watched = true)
        assertEquals("Watched, S2:E1, The North Remembers", TitleCopy.episode(episode))
        assertEquals("S2:E1", TitleCopy.episode(episode.copy(watched = false, title = "")))
    }
}
