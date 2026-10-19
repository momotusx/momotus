package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.model.Blocked
import app.marquee.core.model.Devices
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.DiscoverState
import app.marquee.core.model.HideReason
import app.marquee.core.model.HomeRow
import app.marquee.core.model.ImportSnapshot
import app.marquee.core.model.LostPreview
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.model.PlaybackPhase
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.RowState
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.SyncReport
import app.marquee.core.model.TraktPlayback
import app.marquee.core.model.Tuple2
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

/**
 * Shapes the recorded traces do not reach (they need a failing addon, a torrent, a long download,
 * a sync server), written the way the Rust serde output writes them: camelCase fields, a tag
 * field per union, `null` for an absent value, tuples as arrays. Strict: an unknown key fails.
 */
class ShapeDecodeTest {
    private val strict = Json(CoreJson) { ignoreUnknownKeys = false }

    @Test
    fun `row states carry their reason`() {
        val row =
            strict.decodeFromString(
                HomeRow.serializer(),
                """{"rowId":"a:movie:top","title":"Popular","source":"Cinemeta","tiles":[],
                    "state":{"state":"failed","reason":"timed out"}}""",
            )
        assertEquals(RowState.Failed("timed out"), row.state)
        assertIs<RowState.Slow>(
            strict
                .decodeFromString(
                    HomeRow.serializer(),
                    """{"rowId":"r","title":"t","source":"s","tiles":[],"state":{"state":"slow"}}""",
                ).state,
        )
    }

    @Test
    fun `version rows keep sizes above two gigabytes, every status, and the hidden reasons as tuples`() {
        val snapshot =
            strict.decodeFromString(
                VersionsSnapshot.serializer(),
                """{"recommended":{"key":"k1","quality":"4K","features":["Dolby Vision","Atmos"],"sizeBytes":18253611008,
                      "status":{"kind":"ready","service":"Real-Debrid"},"via":"Real-Debrid","releaseName":"A.2160p.mkv",
                      "languages":["en"],"reasons":["readyNow","matchesScreen"],"group":"best","failoverCount":2,
                      ${VersionsFixtures.ROW_JSON}},
                   "rows":[
                     {"key":"k2","quality":"1080p","features":[],"sizeBytes":null,"status":{"kind":"torrent","seeds":120},
                      "via":"Torrentio","releaseName":"B.1080p","languages":[],"reasons":[],"group":"smallerFaster","failoverCount":0,
                      ${VersionsFixtures.ROW_JSON}},
                     {"key":"k3","quality":"720p","features":[],"sizeBytes":1,"status":{"kind":"needsDownload","service":null},
                      "via":"X","releaseName":"C","languages":["tr"],"reasons":["yourLanguage"],"group":"otherLanguages","failoverCount":0,
                      ${VersionsFixtures.ROW_JSON}},
                     {"key":"k4","quality":"SD","features":[],"sizeBytes":2,"status":{"kind":"direct"},
                      "via":"Y","releaseName":"D","languages":[],"reasons":["playsDirectly"],"group":"needsDownload","failoverCount":0,
                      ${VersionsFixtures.ROW_JSON}}],
                   "hidden":{"count":5,"byReason":[["wrongTitle",3],["tooLarge",2]]},
                   "pendingAddons":["Slow"],"slowAddons":[],"failedAddons":["Broken"],"done":false,"frozen":true,
                   ${VersionsFixtures.SNAPSHOT_JSON}}""",
            )
        assertEquals(18_253_611_008L, snapshot.recommended?.sizeBytes)
        assertEquals(VersionStatus.Ready("Real-Debrid"), snapshot.recommended?.status)
        assertEquals(VersionStatus.Torrent(120), snapshot.rows[0].status)
        assertEquals(VersionStatus.NeedsDownload(null), snapshot.rows[1].status)
        assertEquals(VersionStatus.Direct, snapshot.rows[2].status)
        assertEquals(listOf(Tuple2(HideReason.WrongTitle, 3), Tuple2(HideReason.TooLarge, 2)), snapshot.hidden.byReason)
    }

    @Test
    fun `playback carries attempts, subtitles, the next episode and a progress position`() {
        val snapshot =
            strict.decodeFromString(
                PlaybackSnapshot.serializer(),
                """{"phase":"ready","url":"http://127.0.0.1:1/s/t/x.mkv","key":"k","attempt":2,"manifest":false,"filename":"x.mkv","sizeBytes":5000000000,
                    "resumeMs":4000,"attempts":[{"key":"k0","via":"Local","error":"The link no longer exists (404)"}],
                    "subtitles":[{"url":"https://s/x.srt","lang":"eng","label":null,"source":"OpenSubtitles","forced":false,"hearingImpaired":true}],
                    "subtitlesPending":true,
                    "next":{"videoId":"tt1:1:2","title":"Two","season":1,"episode":2,"thumbnail":null},
                    "bingeGroup":"torrentio|4k",
                    "segments":[{"kind":"intro","startMs":30000,"endMs":75000,"source":"TheIntroDB"},
                                {"kind":"credits","startMs":2400000,"endMs":2520000,"source":"TheIntroDB"}],
                    "nextAtMs":2400000,"trakt":"thisDeviceOff"}""",
            )
        assertEquals(PlaybackPhase.Ready, snapshot.phase)
        assertEquals(TraktPlayback.ThisDeviceOff, snapshot.trakt)
        assertEquals(2L, snapshot.attempt)
        assertEquals(5_000_000_000L, snapshot.sizeBytes)
        assertEquals("OpenSubtitles", snapshot.subtitles.single().source)
        assertEquals(2, snapshot.next?.episode)
        assertEquals(SegmentKind.Credits, snapshot.segments[1].kind)
        assertEquals(2_400_000L, snapshot.nextAtMs)
    }

    @Test
    fun `discover states and the import flow decode`() {
        val end = strict.decodeFromString(DiscoverState.serializer(), """{"state":"end"}""")
        assertEquals(DiscoverState.End, end)
        assertIs<DiscoverState.Failed>(
            strict.decodeFromString(DiscoverState.serializer(), """{"state":"failed","reason":"x"}"""),
        )
        val discover =
            strict.decodeFromString(
                DiscoverSnapshot.serializer(),
                """{"kinds":[["movie","Movies"],["series","Series"]],"kind":"movie","catalogs":[],"catalog":null,"genres":[],
                    "genre":null,"genreRequired":false,"items":[],"state":{"state":"loading"}}""",
            )
        assertEquals(Tuple2("movie", "Movies"), discover.kinds.first())

        val link =
            strict.decodeFromString(
                ImportSnapshot.serializer(),
                """{"phase":"waitingForLink","code":"ABCD","linkUrl":"https://link.example/ABCD","qrUrl":null}""",
            )
        assertIs<ImportSnapshot.WaitingForLink>(link)
        val done =
            strict.decodeFromString(
                ImportSnapshot.serializer(),
                """{"phase":"done","summary":{"addonsImported":3,"addonsSkipped":["X: no manifest"],"titles":10,"inProgress":2,
                    "watched":5,"seriesUnmatched":1}}""",
            )
        assertIs<ImportSnapshot.Done>(done)
    }

    @Test
    fun `a sync report carries counters, why edits were held back, the fingerprint and an optional record`() {
        val report =
            strict.decodeFromString(
                SyncReport.serializer(),
                """{"pushed":3,"stale":1,"refused":0,"waiting":2,"pulled":10,"applied":4,"unreadable":0,"newer":0,
                    "parked":1,"oldEpochWrites":0,"clockGapMs":-1200,"rotationWaitMs":0,"blocked":"rotationPending",
                    "fingerprint":"ab12cd34ef56","record":{"server":"https://sync.example","deviceToken":"secret-token"},
                    "again":true}""",
            )
        assertEquals(-1200L, report.clockGapMs)
        assertEquals(Blocked.RotationPending, report.blocked)
        assertEquals(true, report.again)
        // The record holds keys and a token: it never reaches a log line.
        assertEquals(false, report.toString().contains("secret-token"))
        assertEquals(true, report.toString().contains("record=hidden"))
        val bare =
            strict.decodeFromString(
                SyncReport.serializer(),
                """{"pushed":0,"stale":0,"refused":0,"waiting":0,"pulled":0,"applied":0,"unreadable":0,"newer":0,
                    "parked":0,"oldEpochWrites":0,"clockGapMs":0,"rotationWaitMs":0,"blocked":null,
                    "fingerprint":"","record":null,"again":false}""",
            )
        assertEquals(null, bare.record)
        assertEquals(null, bare.blocked)
    }

    @Test
    fun `the devices screen decodes members and the lost preview`() {
        val devices =
            strict.decodeFromString(
                Devices.serializer(),
                """{"members":[{"id":"a1","kit":false,"name":"Living room","sponsor":null,"addedMs":1700000000000,"thisDevice":true},
                    {"id":"b2","kit":true,"name":null,"sponsor":"a1","addedMs":1700000100000,"thisDevice":false}],
                    "fingerprint":"ab12cd34ef56","epoch":3,"rotationPending":false,"record":null}""",
            )
        assertEquals(3, devices.epoch)
        assertEquals(1_700_000_000_000L, devices.members.first().addedMs)
        assertEquals("a1", devices.members.last().sponsor)
        val preview =
            strict.decodeFromString(LostPreview.serializer(), """{"devices":["b2"],"kits":["k1"]}""")
        assertEquals(listOf("b2"), preview.devices)
    }

    @Test
    fun `pairing phases decode`() {
        val joined =
            strict.decodeFromString(
                PairingSnapshot.serializer(),
                """{"phase":"joined","record":{"server":"s"}}""",
            )
        assertIs<PairingSnapshot.Joined>(joined)
        assertEquals("Joined(record=hidden)", joined.toString()) // never prints the record
        assertIs<PairingSnapshot.Waiting>(
            strict.decodeFromString(
                PairingSnapshot.serializer(),
                """{"phase":"waiting","qr":"mqpair:abc","expiresMs":300000}""",
            ),
        )
        assertEquals(
            PairingSnapshot.Confirm("123456"),
            strict.decodeFromString(PairingSnapshot.serializer(), """{"phase":"confirm","code":"123456"}"""),
        )
        assertEquals(
            PairingSnapshot.Expired,
            strict.decodeFromString(PairingSnapshot.serializer(), """{"phase":"expired"}"""),
        )
        assertEquals(
            PairingSnapshot.Failed("no"),
            strict.decodeFromString(PairingSnapshot.serializer(), """{"phase":"failed","reason":"no"}"""),
        )
    }
}
