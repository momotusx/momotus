package app.marquee.core

import app.marquee.core.model.LostPreview
import app.marquee.core.model.MemberInfo
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.TimeZone
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

class SyncRulesTest {
    private val utc = TimeZone.getTimeZone("UTC")

    private fun member(
        id: String,
        name: String? = id,
        sponsor: String? = null,
        addedMs: Long = 0,
        kit: Boolean = false,
        thisDevice: Boolean = false,
    ) = MemberInfo(id = id, kit = kit, name = name, sponsor = sponsor, addedMs = addedMs, thisDevice = thisDevice)

    @Test
    fun `only a pairing code is taken, and only its server's host is shown`() {
        val code = "marquee:pair?v=2&s=https%3A%2F%2Fsync.example.org%2Frelay&p=abc&k=key&g=sig"
        assertTrue(SyncRules.isPairingCode("  $code\n"))
        assertFalse(SyncRules.isPairingCode("https://example.org/manifest.json"))
        assertEquals("sync.example.org", SyncRules.pairingHost(code))
        assertEquals("127.0.0.1", SyncRules.pairingHost("marquee:pair?v=2&s=http%3A%2F%2F127.0.0.1%3A17803&p=a"))
        assertNull(SyncRules.pairingHost("marquee:pair?v=2&p=abc"))
        assertNull(SyncRules.pairingHost("marquee:pair?v=2&s=not%20an%20address&p=abc"))
    }

    @Test
    fun `the 12 characters read in groups of four`() {
        assertEquals("VW4B V9DJ D5EZ", SyncRules.fingerprintGroups("VW4BV9DJD5EZ"))
        assertEquals(
            "AAAA-BBBB-CCCC-DDDD\nEEEE-FFFF-GGGG-HHHH",
            SyncRules.kitLines("AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG-HHHH"),
        )
    }

    @Test
    fun `this device first, then the others, then the kits, oldest first`() {
        val members =
            listOf(
                member("kit", kit = true, addedMs = 1),
                member("tv", addedMs = 5),
                member("phone", addedMs = 3),
                member("me", addedMs = 9, thisDevice = true),
            )
        assertEquals(listOf("me", "phone", "tv", "kit"), SyncRules.sortMembers(members).map { it.id })
    }

    @Test
    fun `names come from the list, a kit has its own, a gone sponsor reads as removed`() {
        val list =
            listOf(member("phone", "Phone"), member("tv", "TV", sponsor = "phone"), member("x", null, sponsor = "gone"))
        assertEquals("Recovery kit", SyncRules.memberName(member("k", "ignored", kit = true)))
        assertEquals("Unnamed device", SyncRules.memberName(member("n", "")))
        assertEquals("Phone", SyncRules.sponsorName(list[1], list))
        assertEquals("a removed device", SyncRules.sponsorName(list[2], list))
        assertNull(SyncRules.sponsorName(list[0], list))
    }

    @Test
    fun `a lost report drops the lost device and what it added that the user did not untick`() {
        assertEquals(listOf("phone", "tablet"), SyncRules.lostDrop("phone", listOf("tablet", "tv"), keep = setOf("tv")))
    }

    @Test
    fun `kits go unless their maker stays`() {
        val members =
            listOf(
                member("phone"),
                member("tv", sponsor = "phone"),
                member("k1", kit = true, sponsor = "phone"),
                member("k2", kit = true, sponsor = "tv"),
            )
        // The phone is lost and the TV is kept: the phone's kit goes, the TV's stays.
        assertEquals(listOf("k1"), SyncRules.kitsGoing(listOf("k1", "k2"), members, drop = listOf("phone")))
        // A kit the list does not know (made since) goes.
        assertEquals(listOf("k9"), SyncRules.kitsGoing(listOf("k9"), members, drop = listOf("phone")))
    }

    @Test
    fun `the lost page lists every device the preview drops, an unknown one as unknown`() {
        val members = listOf(member("phone", "Phone", addedMs = 1), member("tv", "TV", sponsor = "phone", addedMs = 2))
        val rows = SyncRules.lostRows(LostPreview(devices = listOf("tv", "tablet"), kits = emptyList()), members, utc)
        assertEquals(listOf("tv", "tablet"), rows.map { it.id })
        assertEquals("TV", rows[0].name)
        assertEquals("Added Jan 1, 1970 through Phone", rows[0].detail)
        assertEquals(
            SyncRules.LostRow("tablet", "Unknown device", "Not in this device's list yet. It goes with the others."),
            rows[1],
        )
    }

    @Test
    fun `the newest record wins, by revision`() {
        fun record(revision: Int?) =
            JsonObject(
                if (revision ==
                    null
                ) {
                    emptyMap()
                } else {
                    mapOf("revision" to JsonPrimitive(revision))
                },
            )
        val held = record(5)
        assertSame(held, SyncRules.newer(held, record(4)))
        val next = record(6)
        assertSame(next, SyncRules.newer(held, next))
        val same = record(5)
        assertSame(same, SyncRules.newer(held, same))
        assertSame(next, SyncRules.newer(null, next))
        assertEquals(0, SyncRules.revision(record(null)))
    }

    @Test
    fun `an address with a user name or password is not plain`() {
        assertTrue(SyncRules.plainAddress("https://sync.example.org"))
        assertFalse(SyncRules.plainAddress("https://user:secret@sync.example.org"))
        assertFalse(SyncRules.plainAddress("sync.example.org"))
        assertEquals("sync.example.org", SyncRules.hostOf("https://sync.example.org/x?y"))
    }
}
