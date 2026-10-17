package app.marquee.core

import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.Tuple2
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals
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
    )

    private fun snapshot(
        recommended: VersionRow? = null,
        done: Boolean = false,
        pending: List<String> = emptyList(),
        slow: List<String> = emptyList(),
        failed: List<String> = emptyList(),
    ) = VersionsSnapshot(
        recommended = recommended,
        rows = emptyList(),
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = pending,
        slowAddons = slow,
        failedAddons = failed,
        done = done,
        frozen = false,
    )

    @Test
    fun `a row reads as quality and features, then size and source`() {
        assertEquals("4K, HDR10", VersionsCopy.headline(row()))
        assertEquals("1.3 GB, Real-Debrid", VersionsCopy.source(row()))
        assertEquals("Real-Debrid", VersionsCopy.source(row(sizeBytes = null)))
        assertEquals("4K", VersionsCopy.headline(row(features = emptyList())))
    }

    @Test
    fun `the line under the primary button says what will play, that it is looking, or why nothing plays`() {
        assertEquals(VersionLine.Finding, VersionsCopy.line(null))
        assertEquals(VersionLine.Finding, VersionsCopy.line(snapshot()))
        assertEquals(VersionLine.Problem("Sources could not load"), VersionsCopy.line(null, failed = true))
        val none = VersionsCopy.line(snapshot(done = true))
        assertEquals(true, none is VersionLine.Problem && none.text.startsWith("No playable version found."))
        val chosen = VersionsCopy.line(snapshot(recommended = row())) as VersionLine.Chosen
        assertEquals(listOf("4K", "HDR10", "1.3 GB"), chosen.facts)
        assertEquals("Ready on Real-Debrid", chosen.status)
        assertEquals(true, chosen.good)
        assertEquals("4K, HDR10, 1.3 GB, Ready on Real-Debrid, via Real-Debrid", chosen.spoken)
        val torrent =
            row().copy(
                status = VersionStatus.Torrent(10),
                sizeBytes = null,
                features = emptyList(),
                via = "Torrentio",
            )
        assertEquals(
            VersionLine.Chosen(listOf("4K"), "Torrent, 10 seeds", good = false, via = "Torrentio"),
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
        assertEquals(
            listOf("Best quality", "Smaller and faster", "Other languages", "Needs download"),
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
}
