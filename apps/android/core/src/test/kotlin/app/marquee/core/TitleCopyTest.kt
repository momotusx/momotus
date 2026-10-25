package app.marquee.core

import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
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
import kotlin.test.assertFalse
import kotlin.test.assertTrue

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
            ),
            TitleCopy.facts(details()),
        )
        // The people are links of their own, at most four of the cast and two of each crew.
        assertEquals(
            listOf(
                "Starring" to listOf("A", "B", "C", "D"),
                "Directed by" to listOf("X", "Y"),
                "Written by" to listOf("W"),
            ),
            TitleCopy.people(details().copy(cast = listOf("A", "B", "C", "D", "E"), writers = listOf("W"))),
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
        // Never where the details or the scores came from: those name addons.
        assertEquals(
            emptyList(),
            TitleCopy.facts(
                details(releaseInfo = null, runtime = null)
                    .copy(
                        cast = emptyList(),
                        directors = emptyList(),
                    ),
            ),
        )
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
    fun `with the next episode not out, the page says when it airs where Play would be`() {
        assertEquals("Next episode airs Nov 3, 2026", TitleCopy.nothingToPlay("2026-11-03"))
        assertEquals("Nothing to play yet", TitleCopy.nothingToPlay(null))
    }

    @Test
    fun `Play is the one primary in every state - it plays, plays the kept version, opens Versions, or is inert`() {
        val target = PlayTarget("tt1", "Play", null)
        val play = PrimaryAction("Play")
        assertEquals(play, TitleCopy.primary(target, null))
        assertEquals(play, TitleCopy.primary(target, snapshot()), "still asking")
        assertEquals(play, TitleCopy.primary(target, snapshot(recommended = row(), done = true)))
        assertEquals(play, TitleCopy.primary(target, snapshot(rows = listOf(row()), done = true)), "none ready")
        val empty = { kind: EmptyKind, action: EmptyAction, key: String? ->
            TitleCopy.primary(target, snapshot(done = true).copy(empty = EmptyVersions(kind, action, 0, key = key)))
        }
        assertEquals(play.copy(keptKey = "kept"), empty(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, "kept"))
        // No Settings or account step on the play surface: Play opens Versions, which holds them.
        for ((kind, action) in listOf(
            EmptyKind.NoSource to EmptyAction.SettingsAddons,
            EmptyKind.Unreachable to EmptyAction.TryAgain,
            EmptyKind.NothingPlaysHere to EmptyAction.ChooseVersion,
        )) {
            assertEquals(play, empty(kind, action, null), "$kind: Play opens Versions")
        }
        // A Back-only state keeps Play in its place, inert: a Play that opens a page with only Back is a dead end.
        for (kind in listOf(EmptyKind.NoRoom, EmptyKind.NeedsAccount, EmptyKind.KidsNothing, EmptyKind.NoResults)) {
            val primary = empty(kind, EmptyAction.None, null)
            assertEquals(play.copy(inert = true), primary, "$kind keeps an inert Play")
            assertFalse(TitleCopy.live(primary, target), "$kind: Play does nothing")
        }
        assertTrue(TitleCopy.live(play, target))
        assertFalse(TitleCopy.live(play, null), "no video, no Play")
    }

    @Test
    fun `Play goes to the player at once while versions are searched, to Versions once nothing is recommended`() {
        val target = PlayTarget("tt1", "Play", null)
        val to = { versions: VersionsSnapshot? ->
            TitleCopy.playTo(TitleCopy.primary(target, versions), target, versions)
        }
        // Still searching, or the view not open yet: the player at once, its pick starting when it arrives.
        assertEquals(PlayTo.Player(null), to(null))
        assertEquals(PlayTo.Player(null), to(snapshot()))
        assertEquals(PlayTo.Player("k"), to(snapshot(recommended = row())))
        assertEquals(PlayTo.Versions, to(snapshot(rows = listOf(row()), done = true)), "done, none ready")
        val empty = { kind: EmptyKind, action: EmptyAction, key: String? ->
            snapshot(done = true).copy(empty = EmptyVersions(kind, action, 0, key = key))
        }
        assertEquals(PlayTo.Player("kept"), to(empty(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, "kept")))
        assertEquals(PlayTo.Versions, to(empty(EmptyKind.NoSource, EmptyAction.SettingsAddons, null)))
        assertEquals(PlayTo.Nowhere, to(empty(EmptyKind.NoResults, EmptyAction.None, null)))
        assertEquals(PlayTo.Nowhere, TitleCopy.playTo(PrimaryAction("Play"), null, null), "no video")
    }

    @Test
    fun `the ratings row reads IMDb as x of 10, TMDB and Trakt as percentages, each spoken once`() {
        val items =
            TitleCopy.scoreItems(
                "9.3",
                app.marquee.core.model.TitleScores(
                    app.marquee.core.model.Slot
                        .Shown("87%"),
                    app.marquee.core.model.Slot
                        .Shown("92%"),
                ),
            )
        assertEquals(
            listOf(
                ScoreItem(Mark.Imdb, "9.3/10", "IMDb 9.3 out of 10"),
                ScoreItem(Mark.Tmdb, "87%", "TMDB 87 percent"),
                ScoreItem(Mark.Trakt, "92%", "Trakt 92 percent"),
            ),
            items,
        )
    }

    @Test
    fun `a pending slot keeps its place and a none slot takes none`() {
        val none = app.marquee.core.model.Slot.None
        val pending = app.marquee.core.model.Slot.Pending
        assertEquals(
            listOf(ScoreItem(Mark.Trakt, null, "")),
            TitleCopy.scoreItems(
                null,
                app.marquee.core.model
                    .TitleScores(none, pending),
            ),
        )
        // No token in the build: both none from the first render, nothing reserved.
        assertEquals(
            emptyList(),
            TitleCopy.scoreItems(
                null,
                app.marquee.core.model
                    .TitleScores(none, none),
            ),
        )
        // An older core sends no scores: IMDb alone.
        assertEquals(listOf(Mark.Imdb), TitleCopy.scoreItems("7.4", null).map { it.mark })
    }

    @Test
    fun `a badge links only to an https page on IMDb, TMDB or Trakt`() {
        val items =
            TitleCopy.scoreItems(
                "9.3",
                app.marquee.core.model.TitleScores(
                    app.marquee.core.model.Slot
                        .Shown("87%", "https://www.themoviedb.org/movie/278"),
                    app.marquee.core.model.Slot
                        .Shown("92%", "https://trakt.tv/movies/tt0111161"),
                ),
                "https://www.imdb.com/title/tt0111161/",
            )
        assertEquals(
            listOf(
                "https://www.imdb.com/title/tt0111161/",
                "https://www.themoviedb.org/movie/278",
                "https://trakt.tv/movies/tt0111161",
            ),
            items.map { it.url },
        )
        // Another host, plain http, a script or nothing (a kids profile gets no URL): no link.
        for (bad in listOf(
            "https://evil.example/tt1",
            "http://www.imdb.com/title/tt1/",
            "javascript:alert(1)",
            "https://trakt.tv.evil.example/x",
            "",
            null,
        )) {
            assertEquals(null, TitleCopy.outLink(bad), "$bad")
        }
    }
}
