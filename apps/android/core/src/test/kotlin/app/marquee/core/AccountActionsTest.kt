package app.marquee.core

import app.marquee.core.ScriptedCore.Companion.fail
import app.marquee.core.ScriptedCore.Companion.ok
import app.marquee.core.model.IdentityLink
import app.marquee.core.sync.AccountActions
import app.marquee.core.sync.BrokeredRun
import app.marquee.core.sync.GoogleSheet
import app.marquee.core.sync.PasskeyResult
import app.marquee.core.sync.SignInPlatform
import app.marquee.core.sync.SyncAccount
import app.marquee.core.views.SignInView
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Account's rows (plans/accounts.md 4.3): links, Apple's revoke before unlink and delete, Save a passkey here. */
class AccountActionsTest {
    private val core = ScriptedCore()
    private val server = "https://sync.example.org"
    private val sync = SyncAccount(core.ops, MemoryStore(ScriptedCore.record()), core.scope)

    private class Platform : SignInPlatform {
        val opened = CopyOnWriteArrayList<String>()
        var get: (Boolean) -> PasskeyResult = { PasskeyResult.None }
        var create: () -> PasskeyResult = { PasskeyResult.Used("cred", null, "cHJm") }
        val creates = CopyOnWriteArrayList<String>()
        override val google: GoogleSheet? = null
        override val rpId = "link.example"

        override fun openBrowser(url: String) {
            opened += url
        }

        override suspend fun passkeyGet(
            request: String,
            immediate: Boolean,
        ) = get(immediate)

        override suspend fun passkeyCreate(request: String): PasskeyResult {
            creates += request
            return create()
        }
    }

    private val platform = Platform()

    /** Which views were opened, by name. */
    private val runs = CopyOnWriteArrayList<String>()

    private val actions =
        AccountActions(
            core.ops,
            sync,
            core.scope,
            platform,
            server = { server },
            deviceName = "Pixel",
            ownerName = { "Robin" },
            brokered = { provider, revoke, open ->
                runs += if (revoke) "appleRevoke" else "signIn:$provider"
                BrokeredRun(
                    server,
                    open,
                ) { onPhase ->
                    SignInView(core.client, core.scope, server, provider, revoke, onPhase).also {
                        it.open()
                    }
                }
            },
        )

    init {
        core.answers["relayInfo"] = { ok("""{"providers":["apple","google"]}""") }
        core.answers["identities"] =
            {
                ok(
                    """{"links":[{"link":1,"provider":"apple","createdMs":5},{"link":2,"provider":"google","createdMs":6}]}""",
                )
            }
        core.answers["unlinkIdentity"] = { ok("""{"record":{"revision":2}}""") }
        core.answers["linkIdentity"] = { ok("""{"record":{"revision":2}}""") }
        core.answers["deleteAccount"] = { ok("{}") }
        core.answers["syncDevices"] =
            { ok("""{"members":[],"fingerprint":"FP","epoch":1,"rotationPending":false,"account":"QUNDT1VOVA"}""") }
        core.answers["savePasskey"] = { ok("""{"id":"pk","record":{"revision":3}}""") }
        core.views["signIn"] = { """{"phase":"starting"}""" }
        core.views["appleRevoke"] = { """{"phase":"starting"}""" }
    }

    @AfterTest
    fun stop() = core.close()

    private fun loaded() {
        actions.load()
        core.waitFor("links") { actions.links != null }
    }

    /** Ends the brokered run that opens next with [end], after a good return. */
    private fun finish(
        view: String,
        end: String,
    ) {
        val before = platform.opened.size
        core.push(view, """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        core.waitFor("page") { platform.opened.size > before }
        assertTrue(actions.run!!.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"))
        core.push(view, end)
    }

    private val apple = IdentityLink(1, "apple", 5)
    private val google = IdentityLink(2, "google", 6)

    @Test
    fun `an Apple link is unlinked with Apple's revoke receipt`() {
        loaded()
        actions.unlink(apple)
        core.waitFor("revoke") { runs.contains("appleRevoke") && actions.run != null }
        finish("appleRevoke", """{"phase":"receipt","receipt":"R1","ticket":"T","account":"A"}""")
        core.waitFor("unlinked") { "unlinkIdentity" in core.ops() }
        val unlink = core.last("unlinkIdentity")
        assertEquals("1", unlink.getValue("link").jsonPrimitive.content)
        assertEquals("R1", unlink.getValue("receipt").jsonPrimitive.content)
    }

    @Test
    fun `without Apple's receipt the unlink waits for Unlink anyway, and goes without one`() {
        loaded()
        actions.unlink(apple)
        core.waitFor("revoke") { actions.run != null }
        finish("appleRevoke", """{"phase":"failed","code":"signInRefused"}""")
        core.waitFor("asked") { actions.failure != null }
        assertEquals(AccountCopy.APPLE_REVOKE_FAILED, actions.failure!!.text)
        assertEquals(AccountCopy.UNLINK_ANYWAY, actions.failure!!.actions[0].label)
        assertFalse(actions.deleting, "an unlink's failure shows under Signed in with")
        assertFalse("unlinkIdentity" in core.ops())
        actions.goOn()
        core.waitFor("unlinked") { "unlinkIdentity" in core.ops() }
        assertFalse("receipt" in core.last("unlinkIdentity"))
    }

    @Test
    fun `a Google link unlinks at once`() {
        loaded()
        actions.unlink(google)
        core.waitFor("unlinked") { "unlinkIdentity" in core.ops() }
        assertTrue(runs.isEmpty())
    }

    @Test
    fun `delete with an Apple link sends Apple's receipt, and Delete anyway sends none`() {
        loaded()
        var deleted = 0
        actions.delete { deleted++ }
        core.waitFor("revoke") { actions.run != null }
        finish("appleRevoke", """{"phase":"receipt","receipt":"R2","ticket":"T","account":"A"}""")
        core.waitFor("deleted") { deleted == 1 }
        assertEquals(
            "R2",
            core
                .last("deleteAccount")
                .getValue("receipt")
                .jsonPrimitive.content,
        )
    }

    @Test
    fun `delete without Apple's receipt waits for Delete anyway`() {
        loaded()
        var deleted = 0
        actions.delete { deleted++ }
        core.waitFor("revoke") { actions.run != null }
        finish("appleRevoke", """{"phase":"failed","code":"unreachable"}""")
        core.waitFor("asked") { actions.failure != null }
        assertEquals(AccountCopy.DELETE_ANYWAY, actions.failure!!.actions[0].label)
        assertTrue(actions.deleting, "the deletion's failure shows under Delete account")
        assertFalse("deleteAccount" in core.ops())
        actions.goOn()
        assertTrue(actions.deleting, "Delete anyway is still the deletion")
        core.waitFor("deleted") { deleted == 1 }
        assertFalse("receipt" in core.last("deleteAccount"))
    }

    @Test
    fun `a link that signs in elsewhere offers to move it, and Use it moves it`() {
        loaded()
        core.answers["linkIdentity"] =
            { m -> if ("move" in m) ok("""{"record":{"revision":2}}""") else fail("linkedElsewhere") }
        actions.link("google")
        core.waitFor("run") { actions.run != null }
        finish("signIn", """{"phase":"found","ticket":"T9","account":"OTHER"}""")
        core.waitFor("asked") { actions.failure != null }
        assertEquals("This Google account already signs in to another Momotus account.", actions.failure!!.text)
        actions.goOn()
        core.waitFor("moved") {
            core.asked.any {
                it.getValue("op").jsonPrimitive.content == "linkIdentity" &&
                    "move" in it
            }
        }
    }

    @Test
    fun `Save a passkey here, the passkey already here says so and saves nothing new`() {
        platform.get = { PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm") }
        core.answers["savePasskey"] = { fail("kitHere") }
        actions.savePasskeyHere()
        core.waitFor("said") { actions.said != null }
        assertEquals(AccountCopy.KIT_HERE, actions.said)
        assertTrue(platform.creates.isEmpty(), "no new passkey")
    }

    @Test
    fun `Save a passkey here without one makes it, by the account's id`() {
        actions.savePasskeyHere()
        core.waitFor("saved") { "savePasskey" in core.ops() }
        assertTrue(platform.creates.single().contains("QUNDT1VOVA"))
        assertEquals(
            "Passkey, saved on Pixel",
            core
                .last("savePasskey")
                .getValue("label")
                .jsonPrimitive.content,
        )
    }

    @Test
    fun `after a dropped passkey the save goes straight to a new one`() {
        actions.savePasskeyHere(probe = false)
        core.waitFor("saved") { "savePasskey" in core.ops() }
        assertEquals(1, platform.creates.size)
    }

    @Test
    fun `a full account says so in the shared words`() {
        core.answers["savePasskey"] = { fail("tooManyKits") }
        actions.savePasskeyHere(probe = false)
        core.waitFor("said") { actions.said != null }
        assertEquals(AccountCopy.TOO_MANY_KITS, actions.said)
    }

    @Test
    fun `Save a passkey here passes over another account's passkey and makes one for this account`() {
        platform.get = { PasskeyResult.Used("old", "T1RIRVI", "b2xk") }
        actions.savePasskeyHere()
        core.waitFor("saved") { "savePasskey" in core.ops() && !actions.busy }
        assertEquals(1, platform.creates.size, "a new passkey for this account")
        assertEquals(1, core.ops().count { it == "savePasskey" }, "the other account's passkey is never sent")
        assertEquals(
            "cHJm",
            core
                .last("savePasskey")
                .getValue("prf")
                .jsonPrimitive.content,
        )
        assertNull(actions.said)
    }

    @Test
    fun `a taken passkey is not the end, a new one is made`() {
        platform.get = { PasskeyResult.Used("cred", "QUNDT1VOVA", "b2xk") }
        var saves = 0
        core.answers["savePasskey"] = {
            if (saves++ == 0) fail("kitTaken") else ok("""{"id":"pk","record":{"revision":3}}""")
        }
        actions.savePasskeyHere()
        core.waitFor("saved") { saves == 2 && !actions.busy }
        assertEquals(1, platform.creates.size)
        assertNull(actions.said, "no kitTaken words once the new passkey is saved")
    }

    @Test
    fun `a sign-in another account holds, from a join another way, is offered on Account and moves on Use it`() {
        sync.heldLink = SyncAccount.HeldLink("google", "T3")
        loaded()
        core.waitFor("asked") { actions.failure != null }
        assertEquals(AccountRules.signInFailure("linkedElsewhere", "google"), actions.failure)
        assertNull(sync.heldLink)
        actions.goOn()
        core.waitFor("moved") { "linkIdentity" in core.ops() }
        val moved = core.last("linkIdentity")
        assertEquals("T3", moved.getValue("ticket").jsonPrimitive.content)
        assertTrue(
            moved
                .getValue("move")
                .jsonPrimitive.content
                .toBoolean(),
        )
    }
}
