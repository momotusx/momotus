package app.marquee.core

import kotlin.test.Test
import kotlin.test.assertEquals

class DebridCopyTest {
    @Test
    fun `keys follow the services' order, a blank key removes, an unknown service stays last`() {
        val start = listOf("premiumize" to "p", "elsewhere" to "e")
        val added = DebridCopy.change(start, "torbox", "  t  ")
        assertEquals(listOf("torbox" to "t", "premiumize" to "p", "elsewhere" to "e"), added)
        assertEquals(
            listOf("torbox" to "t2", "premiumize" to "p", "elsewhere" to "e"),
            DebridCopy.change(added, "torbox", "t2"),
        )
        assertEquals(listOf("premiumize" to "p", "elsewhere" to "e"), DebridCopy.change(added, "torbox", " "))
        assertEquals(listOf("torbox" to "t", "elsewhere" to "e"), DebridCopy.change(added, "premiumize", null))
    }

    @Test
    fun `the core's count and the key page read as words`() {
        assertEquals("No debrid service in use.", DebridCopy.inUse(0))
        assertEquals("1 debrid service in use.", DebridCopy.inUse(1))
        assertEquals("2 debrid services in use.", DebridCopy.inUse(2))
        assertEquals("premiumize.me/account", DebridCopy.page(DebridCopy.services[3]))
    }

    @Test
    fun `storing and handing over are told apart`() {
        assertEquals("1 debrid service in use.", DebridCopy.outcome(stored = true, inUse = 1))
        assertEquals(
            "1 debrid service in use. Kept for this session only: this device could not store it.",
            DebridCopy.outcome(stored = false, inUse = 1),
        )
        assertEquals(
            "The core did not take it: closed. Kept for this session only: this device could not store it.",
            DebridCopy.outcome(stored = false, inUse = null, refused = "closed"),
        )
        assertEquals(
            "The core did not take it: closed.",
            DebridCopy.outcome(stored = true, inUse = null, refused = "closed"),
        )
    }
}
