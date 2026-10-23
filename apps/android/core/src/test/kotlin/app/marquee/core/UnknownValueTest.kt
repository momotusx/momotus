package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.FallbackEnumSerializer
import app.marquee.core.model.Group
import app.marquee.core.model.HideReason
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.Reason
import app.marquee.core.model.SubtitleProblem
import app.marquee.core.model.VersionsSnapshot
import kotlinx.serialization.encodeToString
import kotlin.test.Test
import kotlin.test.assertEquals

/** A newer core may send enum values this app does not know yet: they read as Unknown, never fail the message. */
class UnknownValueTest {
    // The recorded Versions update (traces/versions-playback.jsonl), with a newer core's values in it:
    // two new hide reasons, a new group and a new reason on the recommended row.
    private val snapshot =
        """{"done":true,"failedAddons":[],"frozen":false,"nobodySharing":false,""" +
            """"conditions":{"account":false,"engine":false,"freeBytes":null,"hdr":true,""" +
            """"http":{"mbps":15,"samples":0},"minSeeders":50,"network":"other","p2p":{"mbps":10,"samples":0},""" +
            """"runtimeMin":null,"screenClass":2160,"sizeLimitBytes":5368709120},"blocked":null,"note":null,""" +
            """"hidden":{"byReason":[["geoBlocked",2],["tooLarge",1],["expired",1]],"count":4},"pendingAddons":[],""" +
            """"recommended":{"failoverCount":0,"features":[],"group":"cachedElsewhere",""" +
            """"key":"url:http://127.0.0.1:7780/hls/master.m3u8","languages":[],"quality":"1080p",""" +
            """"reasons":["playsDirectly","fastestSeeders"],"releaseName":"Sintel.2010.1080p.local.AES.m3u8",""" +
            """"line":{"kind":"ready","service":null,"sizeBytes":null},"sourceClass":"ready","bitrateMbps":null,""" +
            """"score":{"binge":false,"effectiveHeight":1080,"language":true,"legit":true,"playability":"native",""" +
            """"sizeOk":true,"start":4,"sustains":true,"viable":true,"within":535,"yearOk":true},""" +
            """"sizeBytes":null,"status":{"kind":"direct"},"via":"Local Test"},"rows":[],"slowAddons":[]}"""

    @Test
    fun `a Versions snapshot with a hide reason from a newer core still decodes`() {
        val versions = CoreJson.decodeFromString(VersionsSnapshot.serializer(), snapshot)
        assertEquals(
            listOf(HideReason.Unknown to 2, HideReason.TooLarge to 1, HideReason.Unknown to 1),
            versions.hidden.byReason.map { it.first to it.second },
        )
        assertEquals(Group.Unknown, versions.recommended?.group)
        assertEquals(listOf(Reason.PlaysDirectly, Reason.Unknown), versions.recommended?.reasons)
    }

    private enum class Speed(
        val wire: String,
    ) {
        Fast("fast"),
        Unknown("unknown"),
        ;

        object Serializer : FallbackEnumSerializer<Speed>("Speed", Speed.entries, Speed::wire, Unknown)
    }

    @Test
    fun `an enum with its own unknown value falls back to it`() {
        assertEquals(Speed.Unknown, CoreJson.decodeFromString(Speed.Serializer, "\"unknown\""))
        assertEquals(Speed.Unknown, CoreJson.decodeFromString(Speed.Serializer, "\"warp\""))
        assertEquals(Speed.Fast, CoreJson.decodeFromString(Speed.Serializer, "\"fast\""))
        assertEquals("\"unknown\"", CoreJson.encodeToString(Speed.Serializer, Speed.Unknown))
    }

    @Test
    fun `a playback snapshot decodes without subtitle notes and with a problem from a newer core`() {
        val base =
            """{"phase":"ready","url":"http://127.0.0.1:1/v.mp4","attempt":1,"manifest":false,""" +
                """"subtitlesPending":false,"trakt":"off""""
        assertEquals(emptyList(), CoreJson.decodeFromString(PlaybackSnapshot.serializer(), "$base}").subtitleNotes)
        val notes =
            CoreJson
                .decodeFromString(
                    PlaybackSnapshot.serializer(),
                    """$base,"subtitleNotes":[{"source":"A","problem":"trackFailed"},{"source":"B","problem":"quota"}]}""",
                ).subtitleNotes
        assertEquals(listOf(SubtitleProblem.TrackFailed, SubtitleProblem.Unknown), notes.map { it.problem })
    }

    @Test
    fun `a known value still goes out by its wire name`() {
        assertEquals("\"tooLarge\"", CoreJson.encodeToString(HideReason.TooLarge))
        assertEquals(HideReason.NotForKids, CoreJson.decodeFromString(HideReason.serializer(), "\"notForKids\""))
        // M13's hide reasons are known.
        val m13 = snapshot.replace("geoBlocked", "needsDebrid").replace("expired", "noSpace")
        val known = CoreJson.decodeFromString(VersionsSnapshot.serializer(), m13)
        assertEquals(
            listOf(HideReason.NeedsDebrid, HideReason.TooLarge, HideReason.NoSpace),
            known.hidden.byReason.map { it.first },
        )
        // A zero-playable state from a newer core reads as Unknown, and its line as noResults's.
        val empty =
            snapshot.replace(
                "\"blocked\":null",
                "\"empty\":{\"kind\":\"farAway\",\"action\":\"none\",\"count\":0}",
            )
        assertEquals(EmptyKind.Unknown, CoreJson.decodeFromString(VersionsSnapshot.serializer(), empty).empty?.kind)
    }
}
