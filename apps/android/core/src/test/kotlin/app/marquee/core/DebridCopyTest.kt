package app.marquee.core

import app.marquee.core.model.AccountState
import app.marquee.core.model.DebridLoginSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DebridCopyTest {
    @Test
    fun `the count and the key page read as words`() {
        assertEquals("No debrid service in use.", DebridCopy.inUse(0))
        assertEquals("1 debrid service in use.", DebridCopy.inUse(1))
        assertEquals("2 debrid services in use.", DebridCopy.inUse(2))
        assertEquals("premiumize.me/account", DebridCopy.page(DebridCopy.services[3]))
        assertEquals("real-debrid.com/device", DebridCopy.address("https://real-debrid.com/device/"))
    }

    @Test
    fun `a card says connected only once its record is stored`() {
        assertNull(DebridCopy.state(null, pending = false, durable = true))
        assertEquals("Connected", DebridCopy.state(AccountState.Ok, pending = false, durable = true))
        assertEquals("Saving", DebridCopy.state(AccountState.Ok, pending = true, durable = true))
        assertEquals("Kept for this session", DebridCopy.state(AccountState.Ok, pending = true, durable = false))
        assertEquals(
            "Sign in again: the service asked for it",
            DebridCopy.state(AccountState.Reconnect, pending = false, durable = true),
        )
        assertEquals(
            "Not answering, trying again",
            DebridCopy.state(AccountState.Failing, pending = false, durable = true),
        )
    }

    @Test
    fun `a sign-in counts only for its own new connection, once stored`() {
        assertEquals(true, DebridCopy.signedIn(done = true, generation = "g2", before = "g1", pending = false))
        assertEquals(false, DebridCopy.signedIn(done = true, generation = "g1", before = "g1", pending = false))
        assertEquals(false, DebridCopy.signedIn(done = true, generation = "g2", before = "g1", pending = true))
        assertEquals(true, DebridCopy.signedIn(done = true, generation = "g1", before = null, pending = false))
        assertEquals(false, DebridCopy.signedIn(done = false, generation = "g2", before = "g1", pending = false))
    }

    @Test
    fun `every sign-in phase has its words`() {
        assertEquals("Getting a code", DebridCopy.login(DebridLoginSnapshot.Starting))
        assertEquals("Another sign-in or key took its place.", DebridCopy.login(DebridLoginSnapshot.Used))
        assertEquals("The code ran out before it was entered.", DebridCopy.login(DebridLoginSnapshot.Expired))
        assertEquals("no network", DebridCopy.login(DebridLoginSnapshot.Failed("no network")))
    }
}
