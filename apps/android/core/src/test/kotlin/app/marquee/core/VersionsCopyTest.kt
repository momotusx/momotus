package app.marquee.core

import app.marquee.core.VersionsCopy.EmptyStep
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.Reason
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
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
        // Every addon answered with nothing while the core checks what this device keeps: still finding.
        assertEquals(VersionLine.Finding, VersionsCopy.line(snapshot(done = true)))
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
    fun `the group words read plainly`() {
        assertEquals("Smaller and faster", VersionsCopy.group(Group.SmallerFaster, pickAbove = true))
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
                EmptyKind.entries.map(StreamChoiceCopy::emptyLine) +
                listOf(
                    VersionsCopy.status(VersionStatus.Ready(null)),
                    VersionsCopy.status(VersionStatus.Torrent(12)),
                    VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")),
                    VersionsCopy.status(VersionStatus.Direct),
                )
        assertTrue(words.all { it.isNotBlank() && it.all { c -> c.code < 0x2000 } })
    }

    @Test
    fun `the header never names an addon - finding, still looking, else the count`() {
        assertEquals("Finding versions" to VersionsCopy.Tone.Idle, VersionsCopy.progress(null))
        assertEquals(
            "Finding versions" to VersionsCopy.Tone.Idle,
            VersionsCopy.progress(snapshot(pending = listOf("Torrentio"))),
        )
        val found = snapshot(recommended = row(), pending = listOf("Torrentio"), slow = listOf("Comet"))
        assertEquals("Still looking for more versions" to VersionsCopy.Tone.Idle, VersionsCopy.progress(found))
        assertEquals(
            "1 version" to VersionsCopy.Tone.Ok,
            VersionsCopy.progress(snapshot(recommended = row(), done = true, failed = listOf("Torrentio"))),
        )
        assertEquals("0 versions" to VersionsCopy.Tone.Ok, VersionsCopy.progress(snapshot(done = true)))
        for (s in listOf(
            found,
            snapshot(slow = listOf("Torrentio")),
            snapshot(done = true, failed = listOf("Torrentio")),
        )) {
            val text = VersionsCopy.progress(s)?.first.orEmpty()
            assertFalse("Torrentio" in text || "Comet" in text, text)
        }
    }

    @Test
    fun `nothing that plays says the core's line under Play, and the header counts what was found`() {
        val empty = { kind: EmptyKind, count: Int ->
            snapshot(done = true).copy(empty = EmptyVersions(kind, EmptyAction.None, count))
        }
        assertEquals(
            VersionLine.Problem("These play through a streaming account"),
            VersionsCopy.line(empty(EmptyKind.NeedsAccount, 12)),
        )
        assertEquals("12 versions" to VersionsCopy.Tone.Warn, VersionsCopy.progress(empty(EmptyKind.NeedsAccount, 12)))
        // Nothing found: the empty line says it, so no "0 versions" beside it.
        assertNull(VersionsCopy.progress(empty(EmptyKind.NoResults, 0)))
        // A kind from a newer core still says something.
        assertEquals(VersionLine.Problem("No source has this yet"), VersionsCopy.line(empty(EmptyKind.Unknown, 0)))
    }

    @Test
    fun `each zero-playable state offers its actions in the shared order, Back last`() {
        val steps = { action: EmptyAction -> VersionsCopy.emptySteps(EmptyVersions(EmptyKind.Unknown, action, 0)) }
        assertEquals(
            listOf(EmptyStep.PlayFromDevice, EmptyStep.TryAgain, EmptyStep.Back),
            steps(EmptyAction.PlayFromDevice),
        )
        assertEquals(listOf(EmptyStep.TryAgain, EmptyStep.Back), steps(EmptyAction.TryAgain))
        assertEquals(listOf(EmptyStep.Settings, EmptyStep.Back), steps(EmptyAction.SettingsAddons))
        assertEquals(listOf(EmptyStep.Settings, EmptyStep.Back), steps(EmptyAction.SettingsAccounts))
        // Choose a version is the player's and the title's: Versions lists the rows instead.
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.ChooseVersion))
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.None))
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.Unknown))
        assertEquals(
            listOf("Play from this device", "Try again", "Settings", "Back"),
            EmptyStep.entries.map { it.label },
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
    fun `the player's starting line gives the quality and size, never where from`() {
        val line = VersionsCopy.starting(row().copy(via = "Torrentio", sizeBytes = 2L * 1024 * 1024 * 1024))
        assertEquals("Starting 4K, 2 GB", line)
        assertFalse("Torrentio" in line || " from " in line, line)
        assertEquals("Starting 4K", VersionsCopy.starting(row(sizeBytes = null)))
    }

    @Test
    fun `the card without a pick says none is ready, with no addon name`() {
        assertEquals("Nothing is ready to play yet", VersionsCopy.NOT_READY_TITLE)
        assertEquals("Choose a version to start it.", VersionsCopy.NOT_READY_BODY)
    }

    @Test
    fun `a title page never names an addon`() {
        // The owner, 2026-10-22 (design/DECISION.md, Shelves): addon names only on the Addons pages (Versions names
        // none either since the Versions spec): not the version's addon, nor where the details or the scores came from.
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
