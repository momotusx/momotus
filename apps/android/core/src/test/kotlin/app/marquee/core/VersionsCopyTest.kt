package app.marquee.core

import app.marquee.core.model.BlockedReason
import app.marquee.core.model.BlockedVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.Tuple2
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class VersionsCopyTest {
    private fun row(
        sizeBytes: Long? = 1_300_000_000L,
        features: List<String> = listOf("HDR10"),
    ) = VersionRow(
        key = "k",
        quality = "4K",
        features = features,
        sizeBytes = sizeBytes,
        status = VersionStatus.Ready("Real-Debrid"),
        via = "Real-Debrid",
        releaseName = "A.2160p.mkv",
        languages = emptyList(),
        reasons = listOf(Reason.ReadyNow),
        group = Group.Best,
        failoverCount = 0,
        line = VersionsFixtures.line,
        sourceClass = VersionsFixtures.sourceClass,
        score = VersionsFixtures.score,
    )

    private fun snapshot(
        recommended: VersionRow? = null,
        done: Boolean = false,
        pending: List<String> = emptyList(),
        slow: List<String> = emptyList(),
        failed: List<String> = emptyList(),
    ) = VersionsSnapshot(
        conditions = VersionsFixtures.conditions,
        recommended = recommended,
        rows = emptyList(),
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = pending,
        slowAddons = slow,
        failedAddons = failed,
        done = done,
        frozen = false,
        nobodySharing = false,
    )

    @Test
    fun `a row reads as quality and features, and no fact names its addon`() {
        assertEquals("4K, HDR10", VersionsCopy.headline(row()))
        assertEquals("4K", VersionsCopy.headline(row(features = emptyList())))
        assertTrue(VersionsCopy.facts(row()).none { it.first == "Via" || "Real-Debrid" == it.second })
    }

    @Test
    fun `a row in another language than the viewer's carries its language`() {
        assertEquals(
            "Turkish",
            VersionsCopy.languageTag(row().copy(group = Group.OtherLanguages, languages = listOf("tr"))),
        )
        assertEquals(null, VersionsCopy.languageTag(row().copy(group = Group.Best, languages = listOf("tr"))))
        assertEquals(null, VersionsCopy.languageTag(row().copy(group = Group.OtherLanguages, languages = emptyList())))
    }

    @Test
    fun `the line under the primary button says what will play, that it is looking, or why nothing plays`() {
        assertEquals(VersionLine.Finding, VersionsCopy.line(null))
        assertEquals(VersionLine.Finding, VersionsCopy.line(snapshot()))
        assertEquals(VersionLine.Problem("Sources could not load"), VersionsCopy.line(null, failed = true))
        val none = VersionsCopy.line(snapshot(done = true))
        assertEquals(true, none is VersionLine.Problem && none.text.startsWith("No playable version found."))
        // Every addon answered, rows exist, none is recommended: say so and let the viewer choose.
        val notReady = VersionsCopy.line(snapshot(done = true).copy(rows = listOf(row())))
        assertEquals(VersionLine.Problem("No version is ready to play yet. Choose one to start it."), notReady)
        val chosen = VersionsCopy.line(snapshot(recommended = row())) as VersionLine.Chosen
        assertEquals(listOf("4K", "HDR10", "1.2 GB"), chosen.facts)
        assertEquals("Ready on Real-Debrid", chosen.status)
        assertEquals(true, chosen.good)
        assertEquals("4K, HDR10, 1.2 GB, Ready on Real-Debrid", chosen.spoken)
        val torrent =
            row().copy(
                status = VersionStatus.Torrent(10),
                sizeBytes = null,
                features = emptyList(),
                via = "Torrentio",
            )
        assertEquals(
            VersionLine.Chosen(listOf("4K"), "Torrent, 10 seeds", good = false),
            VersionsCopy.line(snapshot(recommended = torrent)),
        )
    }

    @Test
    fun `an unstated quality is one word in a cell`() {
        assertEquals("Unknown", VersionsCopy.cell("Quality not stated"))
        assertEquals("720p", VersionsCopy.cell("720p"))
        assertEquals("WEB-DL", VersionsCopy.cell("WEB-DL"))
    }

    @Test
    fun `hidden releases are counted, with the reasons in the web's words`() {
        val hidden =
            HiddenSummary(
                4,
                listOf(Tuple2(HideReason.WrongTitle, 3), Tuple2(HideReason.TooLarge, 1)),
            )
        assertEquals("Hidden (4)", VersionsCopy.hiddenTitle(hidden))
        assertEquals("3 are for other titles, 1 is over your size limit", VersionsCopy.hiddenDetail(hidden))
    }

    @Test
    fun `addons that are slow or failed are named`() {
        assertNull(VersionsCopy.addons(snapshot()))
        assertEquals(
            "Waiting for A. Slow: B. No answer from C, D",
            VersionsCopy.addons(snapshot(pending = listOf("A"), slow = listOf("B"), failed = listOf("C", "D"))),
        )
    }

    @Test
    fun `the reasons, groups and status words match the web table`() {
        assertEquals(
            "Ready now, cached on Real-Debrid",
            VersionsCopy.reason(Reason.ReadyNow, VersionStatus.Ready("Real-Debrid")),
        )
        assertEquals("Ready now", VersionsCopy.reason(Reason.ReadyNow))
        // A film replayed and an episode after the last one alike (walk EW-F1).
        assertEquals("Same release as last time", VersionsCopy.reason(Reason.SameReleaseAsLast))
        assertEquals(
            // The four the web names, and the words for a group from a newer core.
            listOf("Best quality", "Smaller and faster", "Other languages", "Needs download", "Other versions"),
            Group.entries.map(VersionsCopy::group),
        )
        assertEquals("Needs download on Real-Debrid", VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")))
        // With the pick above the list, the best group is not "Best quality".
        assertEquals("Other good choices", VersionsCopy.group(Group.Best, pickAbove = true))
        assertEquals("Smaller and faster", VersionsCopy.group(Group.SmallerFaster, pickAbove = true))
    }

    @Test
    fun `every status, reason and group has words and no emoji`() {
        val words =
            Reason.entries.map { VersionsCopy.reason(it) } +
                Group.entries.map(VersionsCopy::group) +
                HideReason.entries.map { VersionsCopy.hiddenDetail(HiddenSummary(2, listOf(Tuple2(it, 2)))) } +
                listOf(
                    VersionsCopy.status(VersionStatus.Ready(null)),
                    VersionsCopy.status(VersionStatus.Torrent(12)),
                    VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")),
                    VersionsCopy.status(VersionStatus.Direct),
                )
        assertTrue(words.all { it.isNotBlank() && it.all { c -> c.code < 0x2000 } })
    }

    @Test
    fun `the header says who is still asked, slow or silent, else the count`() {
        assertEquals("Opening" to VersionsCopy.Tone.Idle, VersionsCopy.progress(null))
        assertEquals(
            "Asking Torrentio" to VersionsCopy.Tone.Warn,
            VersionsCopy.progress(snapshot(pending = listOf("Torrentio"))),
        )
        assertEquals(
            "Torrentio is slow, still waiting" to VersionsCopy.Tone.Warn,
            VersionsCopy.progress(snapshot(slow = listOf("Torrentio"))),
        )
        assertEquals(
            "Torrentio did not answer" to VersionsCopy.Tone.Bad,
            VersionsCopy.progress(snapshot(failed = listOf("Torrentio"))),
        )
        assertEquals(
            "1 version" to VersionsCopy.Tone.Ok,
            VersionsCopy.progress(snapshot(recommended = row(), done = true)),
        )
        assertEquals("0 versions" to VersionsCopy.Tone.Ok, VersionsCopy.progress(snapshot(done = true)))
    }

    @Test
    fun `versions that none of plays here say why under Play and in the header`() {
        val blocked =
            snapshot(done = true).copy(
                blocked = BlockedVersions(BlockedReason.NeedsAccount, 12),
                hidden = HiddenSummary(12, listOf(Tuple2(HideReason.NeedsDebrid, 12))),
            )
        val line = VersionsCopy.line(blocked)
        assertIs<VersionLine.Problem>(line)
        assertTrue(line.text.startsWith("Marquee found 12 versions. This version of the app plays them only through"))
        assertEquals("12 versions" to VersionsCopy.Tone.Warn, VersionsCopy.progress(blocked))
        // A reason from a newer core still says something.
        val unknown = blocked.copy(blocked = BlockedVersions(BlockedReason.Unknown, 3))
        assertEquals(
            VersionLine.Problem(
                "No playable version found. A source is an addon that finds a playable stream for a title.",
            ),
            VersionsCopy.line(unknown),
        )
    }

    @Test
    fun `the card lists the facts it has, in the web's order`() {
        val one = row(sizeBytes = null, features = listOf("HDR10")).copy(languages = listOf("eng", "tur"))
        assertEquals(
            listOf(
                "Size" to "Unknown",
                "Status" to "Ready on Real-Debrid",
                "Languages" to "English, Turkish",
            ),
            VersionsCopy.facts(one),
        )
        val two = row(features = listOf("Dolby Vision", "Atmos"))
        assertEquals(listOf("Size", "Status", "Features"), VersionsCopy.facts(two).map { it.first })
        assertEquals("4K Dolby Vision", VersionsCopy.pickHeading(two))
    }

    @Test
    fun `failover and an opened row say how many sources back a release`() {
        assertEquals("One source for this release.", VersionsCopy.failover(row()))
        assertEquals(
            "3 sources. If one fails, the next starts on its own.",
            VersionsCopy.failover(row().copy(failoverCount = 2)),
        )
        assertEquals("Language not stated", VersionsCopy.detail(row()))
        assertEquals("English, 2 sources", VersionsCopy.detail(row().copy(languages = listOf("en"), failoverCount = 1)))
    }

    @Test
    fun `language codes read as English names, an unknown code in capitals`() {
        assertEquals("English", VersionsCopy.languageName("eng"))
        assertEquals("German", VersionsCopy.languageName("ger"))
        assertEquals("Turkish", VersionsCopy.languageName("tr"))
        assertEquals("Portuguese (Brazil)", VersionsCopy.languageName("pob"))
        assertEquals("QQX", VersionsCopy.languageName("qqx"))
    }

    @Test
    fun `the card without a pick says whether it is looking, nothing came, all was hidden, or none is ready`() {
        val looking = snapshot()
        assertEquals(
            "Finding versions" to "Your addons are answering.",
            VersionsCopy.waitingTitle(looking) to VersionsCopy.waitingBody(looking),
        )
        val none = snapshot(done = true)
        assertEquals("No playable version found", VersionsCopy.waitingTitle(none))
        assertTrue(VersionsCopy.waitingBody(none).startsWith("None of your addons returned a stream"))
        val hidden = none.copy(hidden = HiddenSummary(2, listOf(Tuple2(HideReason.Unplayable, 2))))
        assertEquals("Every result was hidden: 2 cannot play on this device.", VersionsCopy.waitingBody(hidden))
        // Done: why nothing plays comes first, then the addon that did not answer.
        assertEquals(
            "Every result was hidden: 2 cannot play on this device. No answer from Torrentio.",
            VersionsCopy.waitingBody(hidden.copy(failedAddons = listOf("Torrentio"))),
        )
        val unready = none.copy(rows = listOf(row().copy(status = VersionStatus.NeedsDownload("Real-Debrid"))))
        assertEquals(
            "Nothing is ready to play yet" to "Choose a version to start it.",
            VersionsCopy.waitingTitle(unready) to VersionsCopy.waitingBody(unready),
        )
    }

    @Test
    fun `a title page never names an addon, the Versions screens do`() {
        // The owner, 2026-10-22 (design/DECISION.md, Shelves): addon names only on the Addons pages and in Versions:
        // not the version's addon, nor where the details or the scores came from.
        val pages =
            listOf(
                "../ui-tv/src/main/kotlin/app/marquee/ui/tv/TvTitle.kt",
                "../ui-phone/src/main/kotlin/app/marquee/ui/phone/PhoneTitle.kt",
                "src/main/kotlin/app/marquee/core/TitleCopy.kt",
            )
        val names = Regex("""\.via\b|\bvia \$|\.source\b|ratingsFrom|Details from|Ratings from""")
        for (path in pages) {
            assertFalse(names.containsMatchIn(File(path).readText()), "$path names an addon")
        }
    }
}
