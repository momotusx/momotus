package app.marquee.core

import app.marquee.core.model.BlockedReason
import app.marquee.core.model.BlockedVersions
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
        // What the file has left wins over the runtime, rounded up as Continue Watching words it.
        val stored = resume.copy(leftMs = 60_001)
        assertEquals("Resume S1:E2 · 2 min left", TitleCopy.playLabel(stored, "52 min"))
        assertEquals("Resume S1:E2 · 2 min left", TitleCopy.playLabel(stored))
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
            "2010$nbsp· Animation,${nbsp}Short,${nbsp}Fantasy$nbsp· 52${nbsp}min",
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
            "2011-2019$nbsp· Drama$nbsp· 8${nbsp}seasons",
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
    fun `the facts table lists what is known in the web's words`() {
        assertEquals(
            listOf(
                "Released" to "2010",
                "Runtime" to "52 min",
                "Directed by" to "X, Y",
                "Starring" to "A, B, C",
                "Details from" to "Cinemeta",
            ),
            TitleCopy.facts(details()),
        )
        assertEquals(
            listOf("Aired" to "2011-2019", "Seasons" to "8"),
            TitleCopy
                .facts(
                    details(
                        kind = "series",
                        releaseInfo = "2011-2019",
                        runtime = null,
                    ).copy(cast = emptyList(), directors = emptyList()),
                    8,
                ).take(2),
        )
        assertEquals(
            "52 min an episode",
            TitleCopy.facts(details(kind = "series")).first { it.first == "Runtime" }.second,
        )
        assertEquals(
            listOf("Details from" to "Cinemeta"),
            TitleCopy.facts(
                details(releaseInfo = null, runtime = null).copy(cast = emptyList(), directors = emptyList()),
            ),
        )
        // The scores' credit, when MDBList gave one.
        assertEquals(
            "Ratings from" to "IMDb and Rotten Tomatoes via MDBList",
            TitleCopy.facts(details().copy(ratingsFrom = "IMDb and Rotten Tomatoes via MDBList")).last(),
        )
        // Or the Rotten Tomatoes addon, for its Fresh mark.
        val addon = "IMDb from Cinemeta, Rotten Tomatoes from the Rotten Tomatoes addon"
        assertEquals("Ratings from" to addon, TitleCopy.facts(details().copy(ratingsFrom = addon)).last())
    }

    @Test
    fun `an episode reads as watched state, number and title`() {
        val episode = Episode("tt1:2:1", 2, 1, "The North Remembers", watched = true)
        assertEquals("Watched, S2:E1, The North Remembers", TitleCopy.episode(episode))
        assertEquals("S2:E1", TitleCopy.episode(episode.copy(watched = false, title = "")))
    }

    @Test
    fun `an episode card says its number and what is left under its title`() {
        val fresh = Episode("tt1:2:1", 2, 1, "The North Remembers", watched = false)
        assertEquals("S2:E1 · 52 min", TitleCopy.episodeCaption(fresh, "52 min"))
        assertEquals("S2:E1", TitleCopy.episodeCaption(fresh, null))
        val part = fresh.copy(progress = 0.5f)
        assertEquals("S2:E1 · 26 min left", TitleCopy.episodeCaption(part, "52 min"))
        assertEquals("50% watched", TitleCopy.episodeNote(part, null))
        assertEquals("Watched", TitleCopy.episodeNote(fresh.copy(watched = true), "52 min"))
        assertEquals("S2:E1 · 41 min left", TitleCopy.episodeCaption(part.copy(leftMs = 2_460_000), "52 min"))
    }

    @Test
    fun `an episode that has not aired says when it airs, in UTC, and does not count as out`() {
        // 2026-10-20 12:00 UTC.
        val now = 1_792_497_600_000L
        val later = Episode("tt1:3:7", 3, 7, "The Long Night", released = "2026-11-03T00:00:00.000Z", watched = false)
        assertEquals("Airs Nov 3, 2026", TitleCopy.episodeNote(later, "57 min", now))
        assertEquals("S3:E7, The Long Night, Airs Nov 3, 2026", TitleCopy.episode(later, now))
        assertEquals(false, TitleCopy.aired(later, now))
        // A bare date; and west of UTC the day stays the one the release names, not the evening before.
        assertEquals("Airs Nov 3, 2026", TitleCopy.airs(later.copy(released = "2026-11-03"), now))
        val zone = java.util.TimeZone.getDefault()
        try {
            java.util.TimeZone.setDefault(java.util.TimeZone.getTimeZone("America/Los_Angeles"))
            assertEquals("Airs Nov 3, 2026", TitleCopy.episodeNote(later, "57 min", now))
        } finally {
            java.util.TimeZone.setDefault(zone)
        }
        // Out already, or no date: it plays, and says its length.
        val out = later.copy(released = "2026-10-13T00:00:00.000Z")
        assertEquals("57 min", TitleCopy.episodeNote(out, "57 min", now))
        assertEquals(true, TitleCopy.aired(out, now))
        assertEquals(true, TitleCopy.aired(later.copy(released = null), now))
        assertEquals(true, TitleCopy.aired(later.copy(released = "soon"), now))
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
            line = VersionsFixtures.line,
            sourceClass = VersionsFixtures.sourceClass,
            score = VersionsFixtures.score,
        )

    private fun snapshot(
        recommended: VersionRow? = null,
        rows: List<VersionRow> = emptyList(),
        done: Boolean = false,
    ) = VersionsSnapshot(
        conditions = VersionsFixtures.conditions,
        recommended = recommended,
        rows = rows,
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = emptyList(),
        slowAddons = emptyList(),
        failedAddons = emptyList(),
        done = done,
        frozen = false,
        nobodySharing = false,
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

    @Test
    fun `versions that play only through an account make the button Add an account`() {
        val target = PlayTarget("tt1", "Play", null)
        val blocked = { reason: BlockedReason -> snapshot(done = true).copy(blocked = BlockedVersions(reason, 9)) }
        assertEquals(PrimaryAction.AddAccount, TitleCopy.primary(target, blocked(BlockedReason.NeedsAccount)))
        assertEquals("Add an account", PrimaryAction.AddAccount.label)
        // Nothing an account fixes: a source is the way on.
        assertEquals(PrimaryAction.AddSource, TitleCopy.primary(target, blocked(BlockedReason.NothingPlaysHere)))
    }

    @Test
    fun `with the next episode not out, the page says when it airs where Play would be`() {
        assertEquals("Next episode airs Nov 3, 2026", TitleCopy.nothingToPlay("2026-11-03"))
        assertEquals("Nothing to play yet", TitleCopy.nothingToPlay(null))
    }
}
