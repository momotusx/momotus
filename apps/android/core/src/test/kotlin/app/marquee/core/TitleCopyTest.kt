package app.marquee.core

import app.marquee.core.model.Episode
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals

class TitleCopyTest {
    @Test
    fun `the play label says how much is left when the runtime is known, else where playback resumes`() {
        assertEquals("Play", TitleCopy.playLabel(PlayTarget("tt1", "Play", null)))
        assertEquals("Play S1:E1", TitleCopy.playLabel(PlayTarget("tt1:1:1", "Play S1:E1", 0)))
        val resume = PlayTarget("tt1:1:2", "Resume S1:E2", 2_472_000)
        assertEquals("Resume S1:E2 from 41:12", TitleCopy.playLabel(resume))
        assertEquals("Resume S1:E2 · 11 min left", TitleCopy.playLabel(resume, "52 min"))
        // Never "0 min left" while there is something left to watch.
        assertEquals("Resume S1:E2 · 1 min left", TitleCopy.playLabel(resume, "41 min"))
    }

    private fun details(
        kind: String = "movie",
        releaseInfo: String? = "2010",
        runtime: String? = "52 min",
        rating: String? = "7.8",
        genres: List<String> = listOf("Animation", "Short", "Fantasy", "Extra"),
    ) = TitleDetails(
        id = "tt1",
        kind = kind,
        name = "Sintel",
        releaseInfo = releaseInfo,
        runtime = runtime,
        rating = rating,
        genres = genres,
        cast = listOf("A", "B", "C", "D"),
        directors = listOf("X", "Y", "Z"),
        source = "Cinemeta",
        trailers = emptyList(),
    )

    private val nbsp = ' '

    @Test
    fun `the meta line follows the shared order and keeps each item whole`() {
        assertEquals(
            "7.8${nbsp}IMDb$nbsp· 2010$nbsp· Animation,${nbsp}Short,${nbsp}Fantasy$nbsp· 52${nbsp}min",
            TitleCopy.meta(details()),
        )
        assertEquals("2010", TitleCopy.meta(details(runtime = null, rating = null, genres = emptyList())))
        assertEquals(
            "",
            TitleCopy.meta(details(releaseInfo = null, runtime = null, rating = null, genres = emptyList())),
        )
    }

    @Test
    fun `a series says its seasons instead of a runtime`() {
        val series = details(kind = "series", releaseInfo = "2011-2019", rating = "9.2", genres = listOf("Drama"))
        assertEquals(
            "9.2${nbsp}IMDb$nbsp· 2011-2019$nbsp· Drama$nbsp· 8${nbsp}seasons",
            TitleCopy.meta(series, seasons = 8),
        )
        assertEquals(
            "1${nbsp}season",
            TitleCopy.meta(series.copy(releaseInfo = null, rating = null, genres = emptyList()), 1),
        )
        assertEquals(8, TitleCopy.seasonCount(listOf(0, 1, 2, 3, 4, 5, 6, 7, 8)))
    }

    @Test
    fun `seasons run in order with Specials last, as bare numbers when there are many`() {
        assertEquals(listOf(1, 2, 3, 0), TitleCopy.seasonOrder(listOf(0, 3, 1, 2)))
        assertEquals("Season 2", TitleCopy.seasonLabel(2, compact = TitleCopy.compactSeasons(listOf(1, 2))))
        assertEquals("2", TitleCopy.seasonLabel(2, compact = TitleCopy.compactSeasons(listOf(1, 2, 3, 4, 5, 6))))
        assertEquals("Specials", TitleCopy.seasonLabel(0, compact = true))
    }

    @Test
    fun `the details block lists what is known in the web's words`() {
        assertEquals(
            listOf(
                "Starring" to "A, B, C",
                "Directed by" to "X, Y",
                "Runtime" to "52 min",
                "Details from" to "Cinemeta",
            ),
            TitleCopy.facts(details()),
        )
        assertEquals(
            "52 min an episode",
            TitleCopy.facts(details(kind = "series")).first { it.first == "Runtime" }.second,
        )
        assertEquals(
            listOf("Details from" to "Cinemeta"),
            TitleCopy.facts(details(runtime = null).copy(cast = emptyList(), directors = emptyList())),
        )
    }

    @Test
    fun `an episode reads as watched state, number and title`() {
        val episode = Episode("tt1:2:1", 2, 1, "The North Remembers", watched = true)
        assertEquals("Watched, S2:E1, The North Remembers", TitleCopy.episode(episode))
        assertEquals("S2:E1", TitleCopy.episode(episode.copy(watched = false, title = "")))
    }

    @Test
    fun `an episode tile says what is left under its number, then its title`() {
        val fresh = Episode("tt1:2:1", 2, 1, "The North Remembers", watched = false)
        assertEquals("S2:E1 · 52 min\nThe North Remembers", TitleCopy.episodeCaption(fresh, "52 min"))
        assertEquals("S2:E1\nThe North Remembers", TitleCopy.episodeCaption(fresh, null))
        val part = fresh.copy(progress = 0.5f)
        assertEquals("S2:E1 · 26 min left\nThe North Remembers", TitleCopy.episodeCaption(part, "52 min"))
        assertEquals("50% watched", TitleCopy.episodeNote(part, null))
        assertEquals("Watched", TitleCopy.episodeNote(fresh.copy(watched = true), "52 min"))
    }

    private fun row() =
        VersionRow(
            key = "k",
            quality = "1080p",
            features = emptyList(),
            sizeBytes = null,
            status = VersionStatus.Direct,
            via = "X",
            releaseName = "R",
            languages = emptyList(),
            reasons = emptyList(),
            group = Group.Best,
            failoverCount = 0,
        )

    private fun snapshot(
        recommended: VersionRow? = null,
        rows: List<VersionRow> = emptyList(),
        done: Boolean = false,
    ) = VersionsSnapshot(
        recommended = recommended,
        rows = rows,
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = emptyList(),
        slowAddons = emptyList(),
        failedAddons = emptyList(),
        done = done,
        frozen = false,
    )

    @Test
    fun `the primary button never offers Play when nothing can play`() {
        val target = PlayTarget("tt1", "Play", null)
        val play = PrimaryAction.Play("Play")
        assertEquals(play, TitleCopy.primary(target, null))
        assertEquals(play, TitleCopy.primary(target, snapshot()), "still asking: Play stays, the line under it waits")
        assertEquals(play, TitleCopy.primary(target, snapshot(recommended = row(), done = true)))
        assertEquals(PrimaryAction.AddSource, TitleCopy.primary(target, snapshot(done = true)))
        assertEquals(
            PrimaryAction.ChooseVersion,
            TitleCopy.primary(target, snapshot(rows = listOf(row()), done = true)),
        )
        assertEquals(PrimaryAction.Play("Play"), TitleCopy.primary(null, null))
    }
}
