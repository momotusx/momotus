package app.marquee.core

import app.marquee.core.ScriptedCore.Companion.fail
import app.marquee.core.ScriptedCore.Companion.ok
import app.marquee.core.model.WipePending
import app.marquee.core.sync.LibraryRemoval
import app.marquee.core.sync.SyncAccount
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** D13's three steps (plans/accounts.md): the leave with `removeAfter`, Trakt and debrid, then the wipe; resumed at start. */
class LibraryRemovalTest {
    private val core = ScriptedCore()
    private val store = MemoryStore(ScriptedCore.record())
    private val sync = SyncAccount(core.ops, store, core.scope)

    /** Steps 2's work, in the order it ran. */
    private val done = CopyOnWriteArrayList<String>()

    private val removal =
        LibraryRemoval(
            core.ops,
            sync,
            core.scope,
            traktSignOutEvery = { profiles ->
                done += "trakt:" + profiles.joinToString(",")
                true
            },
            debridDisconnectAll = { done += "debrid" },
        )

    @AfterTest
    fun stop() = core.close()

    private fun signOuts(): List<JsonObject> =
        core.asked.filter { it.getValue("op").jsonPrimitive.content == "signOut" }

    @Test
    fun `sign out with removal records it before the leave, then Trakt, debrid and the wipe`() =
        runBlocking {
            removal.signOutAndRemove(force = false, debrid = true, profiles = listOf("default", "kid"))
            val (leave, wipe) = signOuts()
            assertTrue("record" in leave)
            assertEquals(
                "true",
                leave
                    .getValue("removeAfter")
                    .jsonObject
                    .getValue("debrid")
                    .jsonPrimitive.content,
            )
            assertFalse("wipe" in leave)
            assertEquals(listOf("trakt:default,kid", "debrid"), done)
            assertFalse("record" in wipe, "the wipe is sent without a record")
            assertEquals("true", wipe.getValue("wipe").jsonPrimitive.content)
            assertEquals("true", wipe.getValue("disconnectDebrid").jsonPrimitive.content)
            assertNull(store.held, "the record left the keystore with the leave")
            assertTrue(removal.wiped)
        }

    @Test
    fun `unticked debrid keeps the streaming accounts and asks no disconnect`() =
        runBlocking {
            removal.signOutAndRemove(force = false, debrid = false, profiles = listOf("default"))
            assertEquals(listOf("trakt:default"), done)
            assertFalse("disconnectDebrid" in signOuts().last())
        }

    @Test
    fun `the account-deleted card records the removal without a record, then the two steps`() =
        runBlocking {
            store.held = null
            removal.removeHere(debrid = true, profiles = listOf("default"))
            val (record, wipe) = signOuts()
            assertFalse("record" in record)
            assertTrue("removeAfter" in record)
            assertTrue("wipe" in wipe)
        }

    @Test
    fun `a refused wipe says why in the shared words`() {
        core.answers["signOut"] = { m -> if ("wipe" in m) fail("traktConnected") else ok("{}") }
        assertFailsWith<Exception> { runBlocking { removal.signOutAndRemove(false, true, listOf("default")) } }
        assertEquals(AccountCopy.TRAKT_CONNECTED, removal.problem)
        assertFalse(removal.wiped)
    }

    @Test
    fun `a pending removal goes on at start on the owner with no account, once`() {
        store.held = null
        val fresh = SyncAccount(core.ops, store, core.scope)
        val again =
            LibraryRemoval(core.ops, fresh, core.scope, {
                done += "trakt"
                true
            }, { done += "debrid" })
        again.resume(WipePending(debrid = true), owner = false, profiles = listOf("default"))
        Thread.sleep(100)
        assertTrue(done.isEmpty(), "never off the owner profile")
        again.resume(WipePending(debrid = true), owner = true, profiles = listOf("default"))
        core.waitFor("wiped") { again.wiped }
        assertEquals(listOf("trakt", "debrid"), done)
        again.resume(WipePending(debrid = true), owner = true, profiles = listOf("default"))
        Thread.sleep(100)
        assertEquals(1, signOuts().count { "wipe" in it }, "once per launch")
    }

    @Test
    fun `a device that still holds an account never resumes a removal`() {
        removal.resume(WipePending(debrid = false), owner = true, profiles = listOf("default"))
        Thread.sleep(100)
        assertTrue(done.isEmpty())
        assertTrue(signOuts().isEmpty())
    }
}
