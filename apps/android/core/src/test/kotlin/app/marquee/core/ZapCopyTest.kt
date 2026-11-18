package app.marquee.core

import app.marquee.core.model.Dial
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.ZapPick
import app.marquee.core.route.Route
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Zap's route rules (plans/zap.md 2.2, 2.3): the pick's player, the try count, the plain play. */
class ZapCopyTest {
    private val pick =
        ZapPick(
            kind = "series",
            itemId = "tt1",
            videoId = "tt1:2:5",
            season = 2,
            episode = 5,
            name = "Severance",
            episodeName = "Trojan's Horse",
            startMs = 1_234_000,
            from = Dial.Near,
        )

    @Test
    fun `the pick's player carries its start, the tries so far and Discover's kind`() {
        val want = Route.Player("series", "tt1", "tt1:2:5", "Severance", zapAt = 1_234_000, zapTries = 2)
        assertEquals(want.copy(zapKind = "series"), ZapCopy.player(pick, "series", 2))
    }

    @Test
    fun `only a zap that did not play counts a try, every press starts again at 0`() {
        val player = ZapCopy.player(pick, "series", 2)
        assertEquals(Route.Zap("series", 3), ZapCopy.next(player, auto = true))
        assertEquals(Route.Zap("series", 0), ZapCopy.next(player, auto = false))
    }

    @Test
    fun `Start from the beginning is a plain play of the same video from its start`() {
        val plain = ZapCopy.fromStart(ZapCopy.player(pick, null, 1))
        assertEquals(Route.Player("series", "tt1", "tt1:2:5", "Severance", startOver = true), plain)
        assertNull(plain.zapAt)
    }

    @Test
    fun `Zap again on the miss screen deals from 0, never from the tries the route came with`() {
        assertEquals(3, ZapCopy.tries(Route.Zap(null, 3), deals = 0))
        assertEquals(0, ZapCopy.tries(Route.Zap(null, 3), deals = 1))
        assertEquals(0, ZapCopy.tries(Route.Zap(null, 0), deals = 2))
    }

    @Test
    fun `What is this names the episode as S2E5 and its name`() {
        assertEquals("S2:E5 · Trojan's Horse", ZapCopy.episodeLine(2, 5, "Trojan's Horse"))
        assertEquals("S2:E5", ZapCopy.episodeLine(2, 5, null))
        assertNull(ZapCopy.episodeLine(null, null, "x"))
    }

    private fun profiles(
        kids: Boolean,
        session: Boolean = false,
    ) = ProfilesSnapshot(
        profiles = listOf(ProfileInfo("p1", "Ada", "a1", 0, hasPin = false, isOwner = true, syncing = false, kids)),
        active = "p1",
        locked = false,
        max = 5,
        kids = session,
    )

    @Test
    fun `Zap shows only once the profiles are known and never on a kids profile`() {
        assertFalse(ZapCopy.shown(null))
        assertTrue(ZapCopy.shown(profiles(kids = false)))
        assertFalse(ZapCopy.shown(profiles(kids = true)))
        assertFalse(ZapCopy.shown(profiles(kids = false, session = true)))
    }

    @Test
    fun `the third zap in a row that did not play ends the zapping`() {
        assertFalse(ZapCopy.spent(2))
        assertTrue(ZapCopy.spent(3))
    }
}
