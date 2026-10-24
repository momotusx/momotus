package app.marquee.core

import app.marquee.core.model.DownloadBadge
import app.marquee.core.model.DownloadReason
import app.marquee.core.model.DownloadRow
import app.marquee.core.model.DownloadState
import app.marquee.core.model.DownloadStorage
import app.marquee.core.model.DownloadTitle
import app.marquee.core.model.GroupDownload
import app.marquee.core.session.DownloadAction
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DownloadCopyTest {
    private fun badge(
        state: DownloadState,
        reason: DownloadReason? = null,
        progress: Float? = null,
    ) = DownloadBadge("d1", state, reason, mobileData = false, progress = progress, bytesTotal = 2_000_000_000)

    private fun shown(control: DownloadControl) =
        listOf(control.glyph, control.label, control.spoken, control.value, control.ring, control.items, control.hint)

    @Test
    fun `every state gives its glyph, label, spoken word, value, ring and menu (3_1)`() {
        val hint = "Shows download options"
        assertEquals(
            listOf(DownloadGlyph.Download, "Download", "Download", null, null, emptyList<DownloadItem>(), null),
            shown(DownloadCopy.control(null)),
        )
        assertEquals(
            listOf(DownloadGlyph.Clock, "Queued", "Queued", null, 0f, listOf(DownloadItem.Cancel), hint),
            shown(DownloadCopy.control(badge(DownloadState.Queued))),
        )
        assertEquals(
            listOf(
                DownloadGlyph.Stop,
                "45%",
                "Downloading",
                "45 percent",
                0.45f,
                listOf(DownloadItem.Pause, DownloadItem.Cancel),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Running, progress = 0.45f))),
        )
        // No size yet: the word, an empty ring, no value.
        assertEquals(
            listOf(
                DownloadGlyph.Stop,
                "Downloading",
                "Downloading",
                null,
                0f,
                listOf(DownloadItem.Pause, DownloadItem.Cancel),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Running))),
        )
        assertEquals(
            listOf(
                DownloadGlyph.Pause,
                "Paused",
                "Paused",
                "30 percent",
                0.3f,
                listOf(DownloadItem.Resume, DownloadItem.Cancel),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Paused, progress = 0.3f))),
        )
        assertEquals(
            listOf(
                DownloadGlyph.Check,
                "Downloaded",
                "Downloaded",
                null,
                null,
                listOf(DownloadItem.Play, DownloadItem.Delete),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Done, progress = 1f))),
        )
        assertEquals(
            listOf(
                DownloadGlyph.Alert,
                "Couldn't download",
                "Couldn't download",
                null,
                null,
                listOf(DownloadItem.Retry, DownloadItem.Another, DownloadItem.Cancel),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Failed, DownloadReason.Source))),
        )
        assertEquals(
            listOf(
                DownloadGlyph.Alert,
                "Download again",
                "Download again",
                null,
                null,
                listOf(DownloadItem.Again, DownloadItem.Remove),
                hint,
            ),
            shown(DownloadCopy.control(badge(DownloadState.Missing))),
        )
    }

    @Test
    fun `a waiting download says its reason, and only Wi-Fi and Low Data offer mobile data`() {
        val cases =
            listOf(
                DownloadReason.Wifi to
                    Triple(
                        DownloadGlyph.WifiOff,
                        "Waiting for Wi-Fi",
                        listOf(DownloadItem.MobileData, DownloadItem.Cancel),
                    ),
                DownloadReason.LowData to
                    Triple(
                        DownloadGlyph.WifiOff,
                        "Waiting: Low Data Mode is on",
                        listOf(DownloadItem.MobileData, DownloadItem.Cancel),
                    ),
                DownloadReason.Space to Triple(DownloadGlyph.Clock, "Waiting", listOf(DownloadItem.Cancel)),
                DownloadReason.Network to
                    Triple(DownloadGlyph.WifiOff, "Waiting for a connection", listOf(DownloadItem.Cancel)),
                DownloadReason.Playing to
                    Triple(DownloadGlyph.Clock, "Paused while you watch", listOf(DownloadItem.Cancel)),
            )
        for ((reason, want) in cases) {
            val control = DownloadCopy.control(badge(DownloadState.Waiting, reason, 0.2f))
            assertEquals(want, Triple(control.glyph, control.label, control.items), "$reason")
            assertEquals(control.label, control.spoken)
            assertEquals("20 percent", control.value)
        }
    }

    @Test
    fun `each item sends its op, and mobile data releases a waiting one`() {
        val ops = DownloadItem.entries.associateWith(DownloadCopy::op)
        assertEquals(DownloadAction.Pause to false, ops[DownloadItem.Pause])
        assertEquals(DownloadAction.Resume to false, ops[DownloadItem.Resume])
        for (gone in listOf(DownloadItem.Cancel, DownloadItem.Delete, DownloadItem.Remove)) {
            assertEquals(DownloadAction.Remove to false, ops[gone])
        }
        assertEquals(DownloadAction.Retry to false, ops[DownloadItem.Retry])
        assertEquals(DownloadAction.Retry to false, ops[DownloadItem.Again])
        assertEquals(DownloadAction.Retry to true, ops[DownloadItem.MobileData])
        for (elsewhere in listOf(DownloadItem.Start, DownloadItem.Choose, DownloadItem.Another, DownloadItem.Play)) {
            assertNull(ops[elsewhere])
        }
    }

    @Test
    fun `a failure says why in full`() {
        val words =
            listOf(DownloadReason.NoVersion, DownloadReason.Source, DownloadReason.Retries, DownloadReason.NotVideo)
                .map { DownloadCopy.control(badge(DownloadState.Failed, it)).detail }
        assertEquals(
            listOf(
                "No version of this one can be downloaded",
                "The source stopped answering",
                "The download was interrupted too many times",
                "The file wasn't a video",
            ),
            words,
        )
    }

    @Test
    fun `the series menu counts aired episodes and says where the season stands`() {
        val series = GroupDownload(seasons = 3, episodes = 28, downloaded = 0, active = 0)
        assertEquals(
            listOf(
                GroupItem("Download Season 1 (12 episodes)", GroupAct.DownloadSeason),
                GroupItem("Download every season (3 seasons, 28 episodes)", GroupAct.DownloadSeries),
            ),
            DownloadCopy.seriesMenu(1, GroupDownload(1, 12, 0, 0), series),
        )
        // Some of the season already kept: the rest, and its delete.
        assertEquals(
            listOf(
                GroupItem("Download Season 1 (1 episode)", GroupAct.DownloadSeason),
                GroupItem("Delete Season 1 downloads", GroupAct.DeleteSeason),
                GroupItem("Download every season (3 seasons, 28 episodes)", GroupAct.DownloadSeries),
            ),
            DownloadCopy.seriesMenu(1, GroupDownload(1, 12, 11, 0), series.copy(downloaded = 11)),
        )
        assertEquals(
            GroupItem("Downloading Season 2", GroupAct.None),
            DownloadCopy.seriesMenu(2, GroupDownload(1, 8, 8, 3), series).first(),
        )
        assertEquals(
            GroupItem("Season 2 downloaded", GroupAct.None),
            DownloadCopy.seriesMenu(2, GroupDownload(1, 8, 8, 0), series).first(),
        )
        // One season, or everything kept: no "every season".
        assertEquals(
            listOf(GroupItem("Download Season 1 (8 episodes)", GroupAct.DownloadSeason)),
            DownloadCopy.seriesMenu(1, GroupDownload(1, 8, 0, 0), GroupDownload(1, 8, 0, 0)),
        )
    }

    @Test
    fun `the downloads screen's lines`() {
        val gb = 1L shl 30
        assertEquals(
            "Downloads use 12.4 GB · 48 GB free on this device",
            DownloadCopy.storage(DownloadStorage((12.4 * gb).toLong(), 48 * gb)),
        )
        assertEquals("Downloads use 300 MB", DownloadCopy.storage(DownloadStorage(300L shl 20)))
        assertEquals("1.2 of 2.1 GB", DownloadCopy.ofTotal((1.2 * gb).toLong(), (2.1 * gb).toLong()))
        assertEquals("300 MB of 2.1 GB", DownloadCopy.ofTotal(300L shl 20, (2.1 * gb).toLong()))
        assertEquals("6 min left", DownloadCopy.left(330))
        assertEquals("1 h 5 min left", DownloadCopy.left(3_900))
        assertEquals("Less than a minute left", DownloadCopy.left(20))
        val row =
            DownloadRow(
                id = "d1",
                kind = "series",
                itemId = "tt1",
                videoId = "tt1:1:3",
                season = 1,
                episode = 3,
                name = "Severance",
                episodeTitle = "In Perpetuity",
                runtime = "52 min",
                state = DownloadState.Running,
                mobileData = false,
                bytesDone = (1.2 * gb).toLong(),
                bytesTotal = (2.1 * gb).toLong(),
                etaSecs = 330,
                quality = "1080p",
            )
        assertEquals("1.2 of 2.1 GB · 6 min left", DownloadCopy.progressLine(row))
        assertEquals(
            "1.2 of 2.1 GB · Waiting for Wi-Fi",
            DownloadCopy.progressLine(row.copy(state = DownloadState.Waiting, reason = DownloadReason.Wifi)),
        )
        assertEquals("E3 In Perpetuity", DownloadCopy.episodeName(row))
        assertEquals("52 min · 2.1 GB", DownloadCopy.episodeLine(row))
        val film =
            DownloadTitle("tt2", "movie", "Sintel", null, 1, (2.1 * gb).toLong(), listOf(row.copy(kind = "movie")))
        assertEquals("1080p · 2.1 GB", DownloadCopy.titleLine(film))
        assertEquals("4 episodes · 2.1 GB", DownloadCopy.titleLine(film.copy(kind = "series", count = 4)))
        assertEquals("Delete 4 downloaded episodes of Severance?", DownloadCopy.deleteSeries(4, "Severance"))
        assertEquals("Delete all downloads on this device? 12 GB", DownloadCopy.deleteAll(12 * gb))
    }
}
