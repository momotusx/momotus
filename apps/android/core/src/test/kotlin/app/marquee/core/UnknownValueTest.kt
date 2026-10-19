package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.model.Group
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.VersionsSnapshot
import kotlinx.serialization.encodeToString
import kotlin.test.Test
import kotlin.test.assertEquals

/** A newer core may send enum values this app does not know yet: they read as Unknown, never fail the message. */
class UnknownValueTest {
    // The recorded Versions update (traces/versions-playback.jsonl), with a newer core's values in it:
    // two new hide reasons, a new group and a new reason on the recommended row.
    private val snapshot =
        """{"done":true,"failedAddons":[],"frozen":false,""" +
            """"hidden":{"byReason":[["needsDebrid",2],["tooLarge",1],["noSpace",1]],"count":4},"pendingAddons":[],""" +
            """"recommended":{"failoverCount":0,"features":[],"group":"cachedElsewhere",""" +
            """"key":"url:http://127.0.0.1:7780/hls/master.m3u8","languages":[],"quality":"1080p",""" +
            """"reasons":["playsDirectly","fastestSeeders"],"releaseName":"Sintel.2010.1080p.local.AES.m3u8",""" +
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
        assertEquals(
            "2 are hidden for another reason, 1 is over your size limit, 1 is hidden for another reason",
            VersionsCopy.hiddenDetail(versions.hidden),
        )
    }

    @Test
    fun `a known value still goes out by its wire name`() {
        assertEquals("\"tooLarge\"", CoreJson.encodeToString(HideReason.TooLarge))
        assertEquals(HideReason.NotForKids, CoreJson.decodeFromString(HideReason.serializer(), "\"notForKids\""))
    }
}
