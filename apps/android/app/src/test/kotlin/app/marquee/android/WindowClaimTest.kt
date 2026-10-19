package app.marquee.android

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A player that replaces a player keeps the window: the old one's leaving puts nothing back. */
class WindowClaimTest {
    @Test
    fun `the old player leaving after the new one came in leaves the window to the new one`() {
        val claim = WindowClaim()
        val old = Any()
        val new = Any()
        claim.claim(old)
        assertTrue(claim.owns(old))
        // The next episode or an offered version comes in while the old player fades out.
        claim.claim(new)
        assertFalse(claim.owns(old), "the old player must not put the window back")
        claim.release(old)
        assertTrue(claim.owns(new))
        // The last player leaving puts it back.
        claim.release(new)
        assertFalse(claim.owns(new))
    }
}
