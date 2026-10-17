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
    fun `the line under Play says what will play or why nothing is ready`() {
        assertEquals("Looking for sources", VersionsCopy.chosen(null))
        assertEquals("Looking for sources", VersionsCopy.chosen(snapshot()))
        assertEquals("No playable source found", VersionsCopy.chosen(snapshot(done = true)))
        assertEquals("4K, HDR10, 1.3 GB, Real-Debrid", VersionsCopy.chosen(snapshot(recommended = row())))
    }

    @Test
    fun `hidden releases are counted by reason`() {
        assertNull(VersionsCopy.hidden(HiddenSummary(0, emptyList())))
        assertEquals(
            "5 hidden: 3 wrong title, 2 too large",
            VersionsCopy.hidden(
                HiddenSummary(5, listOf(Tuple2(HideReason.WrongTitle, 3), Tuple2(HideReason.TooLarge, 2))),
            ),
        )
    }

    @Test
    fun `add-ons that are slow or failed are named`() {
        assertNull(VersionsCopy.addons(snapshot()))
        assertEquals(
            "Waiting for A. Slow: B. No answer from C, D",
            VersionsCopy.addons(snapshot(pending = listOf("A"), slow = listOf("B"), failed = listOf("C", "D"))),
        )
    }

    @Test
    fun `every status, reason and group has words and no emoji`() {
        val words =
            Reason.entries.map(VersionsCopy::reason) +
                Group.entries.map(VersionsCopy::group) +
                HideReason.entries.map(VersionsCopy::hiddenReason) +
                listOf(
                    VersionsCopy.status(VersionStatus.Ready(null)),
                    VersionsCopy.status(VersionStatus.Torrent(12)),
                    VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")),
                    VersionsCopy.status(VersionStatus.Direct),
                )
        assertTrue(words.all { it.isNotBlank() && it.all { c -> c.code < 0x2000 } })
    }
}
