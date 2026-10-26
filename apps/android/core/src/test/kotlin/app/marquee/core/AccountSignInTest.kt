package app.marquee.core

import app.marquee.core.AccountRules.Act
import app.marquee.core.ScriptedCore.Companion.fail
import app.marquee.core.ScriptedCore.Companion.ok
import app.marquee.core.sync.AccountSignIn
import app.marquee.core.sync.AccountSignIn.Step
import app.marquee.core.sync.BrokeredRun
import app.marquee.core.sync.GoogleAnswer
import app.marquee.core.sync.GoogleSheet
import app.marquee.core.sync.PasskeyResult
import app.marquee.core.sync.SignInPlatform
import app.marquee.core.sync.SyncAccount
import app.marquee.core.views.SignInView
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** A phone's sign-in (plans/accounts.md 4.1, 5) against a scripted core and a stand-in for the platform's sheets. */
class AccountSignInTest {
    private val core = ScriptedCore()
    private val server = "https://sync.example.org"

    private class Platform : SignInPlatform {
        val opened = CopyOnWriteArrayList<String>()
        val gets = CopyOnWriteArrayList<Pair<String, Boolean>>()
        val creates = CopyOnWriteArrayList<String>()
        var get: (Boolean) -> PasskeyResult = { PasskeyResult.None }
        var create: () -> PasskeyResult = { PasskeyResult.Cancelled }
        override var google: GoogleSheet? = null
        override val rpId = "link.example"

        override fun openBrowser(url: String) {
            opened += url
        }

        override suspend fun passkeyGet(
            request: String,
            immediate: Boolean,
        ): PasskeyResult {
            gets += request to immediate
            return get(immediate)
        }

        override suspend fun passkeyCreate(request: String): PasskeyResult {
            creates += request
            return create()
        }
    }

    private val platform = Platform()
    private val sync = SyncAccount(core.ops, MemoryStore(), core.scope)

    private fun signIn(nativeGoogle: Boolean = false) =
        AccountSignIn(
            core.ops,
            sync,
            core.scope,
            platform,
            server,
            "Pixel",
            "phone",
            ownerName = { "Robin" },
            brokered = { provider, open ->
                BrokeredRun(server, open) { onPhase ->
                    SignInView(core.client, core.scope, server, provider, false, onPhase).also { it.open() }
                }
            },
            nativeGoogle = nativeGoogle,
        )

    init {
        core.answers["relayInfo"] = { ok("""{"providers":["mock","google"]}""") }
        core.views["signIn"] = { """{"phase":"starting"}""" }
        core.answers["createSyncAccount"] = { ok("""{"revision":1}""") }
        core.answers["recover"] = { ok("""{"record":{"revision":1},"via":"passkey"}""") }
        core.answers["linkIdentity"] = { ok("""{"record":{"revision":2}}""") }
        core.answers["syncDevices"] =
            { ok("""{"members":[],"fingerprint":"FP","epoch":1,"rotationPending":false,"account":"QUNDT1VOVA"}""") }
        core.answers["savePasskey"] = { ok("""{"id":"pk1","record":{"revision":3}}""") }
    }

    @AfterTest
    fun stop() = core.close()

    private fun ready(s: AccountSignIn) {
        s.start()
        core.waitFor("providers") { s.step is Step.Start }
    }

    /** Runs the brokered flow of [provider] to its browser phase, then ends it with [end] after a good return. */
    private fun broker(
        s: AccountSignIn,
        end: String,
        provider: String = "mock",
    ) {
        val pages = platform.opened.size
        val views = core.ops().count { it == "subscribe" }
        s.choose(provider)
        core.waitFor("the view") { core.ops().count { it == "subscribe" } > views && s.run != null }
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/v1/auth/mock/go","expiresMs":9}""")
        core.waitFor("the page opened") { platform.opened.size > pages }
        assertTrue(s.run!!.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"))
        core.push("signIn", end)
    }

    @Test
    fun `providers come in Android's order, and a relay that does not answer says so`() {
        val s = signIn()
        ready(s)
        assertEquals(listOf("google", "mock"), (s.step as Step.Start).providers)
        core.answers["relayInfo"] = { fail(null, "offline") }
        val down = signIn()
        down.start()
        core.waitFor("failure") { down.failure != null }
        assertEquals(AccountCopy.UNREACHABLE, down.failure!!.text)
        assertEquals(listOf(Act.Retry, Act.NotNow), down.failure!!.actions.map { it.act })
        // Try again with no provider chosen yet asks the relay again.
        val asked = core.ops().count { it == "relayInfo" }
        core.answers["relayInfo"] = { ok("""{"providers":["mock","google"]}""") }
        down.act(Act.Retry)
        core.waitFor("providers") { (down.step as? Step.Start)?.providers?.isNotEmpty() == true }
        assertEquals(asked + 1, core.ops().count { it == "relayInfo" })
        assertNull(down.failure)
    }

    @Test
    fun `a brokered return is taken only while it waits, only from marquee auth, and only for this relay`() {
        val s = signIn()
        ready(s)
        s.choose("mock")
        core.waitFor("the view") { s.run != null }
        val run = s.run!!
        assertFalse(run.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"), "no page opened yet")
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        core.waitFor("opened") { platform.opened.size == 1 }
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        Thread.sleep(50)
        assertEquals(1, platform.opened.size, "the page opens once")
        assertFalse(run.returned("https://evil.example/auth#flow=f&code=c"), "only the app's own return")
        assertFalse(run.returned("momotus://other#flow=f&code=c"))
        assertFalse(run.returned("momotus://auth#flow=f&code=c&relay=evil.example"), "another relay's return")
        assertTrue(core.commands.isEmpty(), "nothing reached the core")
        assertTrue(run.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"))
        core.waitFor("command") { core.commands.isNotEmpty() }
        val (view, command) = core.commands.single()
        assertEquals("signIn", view)
        assertEquals("returned", command.getValue("type").jsonPrimitive.content)
        assertEquals("flow=f&code=c&relay=sync.example.org", command.getValue("fragment").jsonPrimitive.content)
        val params = core.last("subscribe").getValue("params").jsonObject
        assertEquals("momotus://auth", params.getValue("return").jsonPrimitive.content)
    }

    @Test
    fun `found with this account's passkey opens it with the PRF output`() {
        platform.get = { PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"found","ticket":"T1","account":"4143434f554e54"}""")
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertTrue(platform.gets.single().second, "an immediate request: only a passkey already here")
        val recover = core.last("recover")
        assertEquals("cHJm", recover.getValue("prf").jsonPrimitive.content)
        assertEquals("phone", recover.getValue("form").jsonPrimitive.content)
        assertTrue(sync.joined)
    }

    @Test
    fun `found with another account's passkey never recovers into it`() {
        platform.get = { PasskeyResult.Used("cred", "T1RIRVI", "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"found","ticket":"T1","account":"4143434f554e54"}""")
        core.waitFor("found") { s.step is Step.Found }
        assertFalse("recover" in core.ops())
        assertEquals(AccountCopy.KIT_UNKNOWN_PASSKEY, s.failure!!.text)
        assertEquals(listOf(Act.Kit, Act.CantUse), s.failure!!.actions.map { it.act })
    }

    @Test
    fun `found with no passkey, or kitUnknown, goes to the QR, a kit or Start over`() {
        core.answers["recover"] = { fail("kitUnknown") }
        platform.get = { PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"found","ticket":"T1","account":"4143434f554e54"}""")
        core.waitFor("found") { s.step is Step.Found }
        assertEquals(AccountCopy.KIT_UNKNOWN_PASSKEY, s.failure!!.text)
        s.act(Act.CantUse)
        assertEquals(Step.StartOverAsk, s.step)
        s.act(Act.Cancel)
        assertIs<Step.Found>(s.step, "Cancel on the warning keeps the found account")
        assertEquals(listOf(Act.Kit, Act.CantUse), s.failure!!.actions.map { it.act })
        s.act(Act.CantUse)
        s.confirmStartOver()
        core.waitFor("created") { "createSyncAccount" in core.ops() }
        val create = core.last("createSyncAccount")
        assertEquals("T1", create.getValue("ticket").jsonPrimitive.content)
        assertTrue(
            create
                .getValue("move")
                .jsonPrimitive.content
                .toBoolean(),
            "Start over moves the sign-in",
        )
    }

    @Test
    fun `new, created with the ticket, then its passkey by the account's id`() {
        platform.create = { PasskeyResult.Used("cred", null, "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T2"}""")
        core.waitFor("new") { s.step == Step.New }
        s.create()
        core.waitFor("in") { s.step == Step.Done(created = true) }
        val create = core.last("createSyncAccount")
        assertEquals("T2", create.getValue("ticket").jsonPrimitive.content)
        assertEquals("phone", create.getValue("form").jsonPrimitive.content)
        val made = Json.parseToJsonElement(platform.creates.single()).jsonObject
        assertEquals(
            "QUNDT1VOVA",
            made
                .getValue("user")
                .jsonObject
                .getValue("id")
                .jsonPrimitive.content,
        )
        assertEquals(
            "Robin (Momotus)",
            made
                .getValue("user")
                .jsonObject
                .getValue("name")
                .jsonPrimitive.content,
        )
        val save = core.last("savePasskey")
        assertEquals("cHJm", save.getValue("prf").jsonPrimitive.content)
        assertEquals("Passkey, saved on Pixel", save.getValue("label").jsonPrimitive.content)
        assertNull(s.held, "the ticket leaves memory once used")
    }

    @Test
    fun `a new passkey without a PRF result asks once for that passkey`() {
        platform.create = { PasskeyResult.Used("newcred", null, null) }
        platform.get =
            { immediate -> if (immediate) PasskeyResult.None else PasskeyResult.Used("newcred", "QUNDT1VOVA", "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T2"}""")
        core.waitFor("new") { s.step == Step.New }
        s.create()
        core.waitFor("in") { s.step == Step.Done(created = true) }
        val follow = Json.parseToJsonElement(platform.gets.last().first).jsonObject
        val allowed =
            follow
                .getValue(
                    "allowCredentials",
                ).jsonArray
                .map {
                    it.jsonObject
                        .getValue("id")
                        .jsonPrimitive.content
                }
        assertEquals(listOf("newcred"), allowed)
        assertFalse(platform.gets.last().second, "the follow-up may show its sheet")
        assertEquals(
            "cHJm",
            core
                .last("savePasskey")
                .getValue("prf")
                .jsonPrimitive.content,
        )
    }

    @Test
    fun `no PRF or a cancelled sheet after creating is the second-way step`() {
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T2"}""")
        core.waitFor("new") { s.step == Step.New }
        s.create()
        core.waitFor("second way") { s.step == Step.SecondWay }
        assertFalse("savePasskey" in core.ops())
        s.later()
        assertEquals(Step.Done(created = true), s.step)
    }

    @Test
    fun `new with a Momotus passkey here signs in with it and links the identity`() {
        platform.get = { PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T3"}""")
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertEquals(
            "T3",
            core
                .last("linkIdentity")
                .getValue("ticket")
                .jsonPrimitive.content,
        )
        assertFalse("createSyncAccount" in core.ops())
    }

    @Test
    fun `linkedElsewhere on Create is the found screen`() {
        core.answers["createSyncAccount"] = { fail("linkedElsewhere") }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T2"}""")
        core.waitFor("new") { s.step == Step.New }
        s.create()
        core.waitFor("found") { s.step is Step.Found }
    }

    @Test
    fun `an expired ticket says so, and Continue signs in again and creates with the new one`() {
        var creates = 0
        core.answers["createSyncAccount"] = { m ->
            creates++
            if (m.getValue("ticket").jsonPrimitive.content == "OLD") fail("ticketExpired") else ok("""{"revision":1}""")
        }
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"OLD"}""")
        core.waitFor("new") { s.step == Step.New }
        s.create()
        core.waitFor("failure") { s.failure != null }
        assertEquals(AccountCopy.TICKET_EXPIRED, s.failure!!.text)
        platform.opened.clear()
        s.act(Act.Retry)
        core.waitFor("the second view") { core.ops().count { it == "subscribe" } == 2 && s.run != null }
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        core.waitFor("opened") { platform.opened.isNotEmpty() }
        assertTrue(s.run!!.returned("momotus://auth#flow=g&code=d&relay=sync.example.org"))
        core.push("signIn", """{"phase":"new","ticket":"NEW"}""")
        core.waitFor("created again") { creates == 2 }
        assertEquals(
            "NEW",
            core
                .last("createSyncAccount")
                .getValue("ticket")
                .jsonPrimitive.content,
        )
    }

    @Test
    fun `I already use Momotus links the new identity once the QR joined, a found one is not linked again`() {
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T4"}""")
        core.waitFor("new") { s.step == Step.New }
        // The pairing screen joined this device (its record stored), then tells the sign-in.
        sync.join(ScriptedCore.record()).let { j -> kotlinx.coroutines.runBlocking { j.join() } }
        s.joined()
        core.waitFor("linked") { "linkIdentity" in core.ops() }
        assertEquals(
            "T4",
            core
                .last("linkIdentity")
                .getValue("ticket")
                .jsonPrimitive.content,
        )
        val f = signIn()
        ready(f)
        broker(f, """{"phase":"found","ticket":"T5","account":"4143434f554e54"}""")
        core.waitFor("found") { f.step is Step.Found }
        val before = core.ops().count { it == "linkIdentity" }
        f.joined()
        core.waitFor("in") { f.step == Step.Done(created = false) }
        assertEquals(before, core.ops().count { it == "linkIdentity" })
    }

    @Test
    fun `Google is one sheet with the relay's bound nonce, and its ID token goes to nativeSignIn`() {
        val nonces = CopyOnWriteArrayList<String>()
        val passkeys = CopyOnWriteArrayList<String?>()
        platform.google =
            object : GoogleSheet {
                override suspend fun signIn(
                    nonce: String,
                    passkeyRequest: String?,
                ): GoogleAnswer {
                    nonces += nonce
                    passkeys += passkeyRequest
                    return GoogleAnswer.IdToken("ID-TOKEN")
                }
            }
        core.answers["nativeSignInStart"] =
            { ok("""{"flow":"FLOW","nonce":"4ddb66d1","expiresMs":${Long.MAX_VALUE}}""") }
        core.answers["nativeSignIn"] = { ok("""{"phase":"new","ticket":"T6"}""") }
        val s = signIn(nativeGoogle = true)
        ready(s)
        s.choose("google")
        core.waitFor("new") { s.step == Step.New }
        assertEquals(listOf("4ddb66d1"), nonces.distinct())
        assertTrue(passkeys.single()!!.contains("bWFycXVlZS9wYXNza2V5L3Yx"), "the passkey rides in the same sheet")
        val native = core.last("nativeSignIn")
        assertEquals("FLOW", native.getValue("flow").jsonPrimitive.content)
        assertEquals("ID-TOKEN", native.getValue("idToken").jsonPrimitive.content)
        assertFalse("subscribe" in core.ops(), "no brokered flow")
    }

    @Test
    fun `the passkey picked in Google's sheet opens its account at once`() {
        platform.google =
            object : GoogleSheet {
                override suspend fun signIn(
                    nonce: String,
                    passkeyRequest: String?,
                ) = GoogleAnswer.Passkey(PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm"))
            }
        core.answers["nativeSignInStart"] = { ok("""{"flow":"FLOW","nonce":"n","expiresMs":${Long.MAX_VALUE}}""") }
        val s = signIn(nativeGoogle = true)
        ready(s)
        s.choose("google")
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertEquals(
            "cHJm",
            core
                .last("recover")
                .getValue("prf")
                .jsonPrimitive.content,
        )
        assertFalse("nativeSignIn" in core.ops())
    }

    @Test
    fun `a cancelled provider says nothing and leaves the sign-in screen`() {
        platform.google =
            object : GoogleSheet {
                override suspend fun signIn(
                    nonce: String,
                    passkeyRequest: String?,
                ) = GoogleAnswer.Cancelled
            }
        core.answers["nativeSignInStart"] = { ok("""{"flow":"FLOW","nonce":"n","expiresMs":${Long.MAX_VALUE}}""") }
        val s = signIn(nativeGoogle = true)
        ready(s)
        s.choose("google")
        core.waitFor("back") { s.step is Step.Start && "nativeSignInStart" in core.ops() }
        Thread.sleep(50)
        assertNull(s.failure)
    }

    @Test
    fun `back in front with no return asks Didn't finish signing in, and the flow keeps waiting`() {
        val s = signIn()
        ready(s)
        s.choose("mock")
        core.waitFor("view") { s.run != null }
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        core.waitFor("opened") { platform.opened.isNotEmpty() }
        s.resumed()
        core.waitFor("asked", timeoutMs = 4_000) { s.failure != null }
        assertEquals(AccountCopy.NO_REDIRECT, s.failure!!.text)
        assertTrue(
            s.run!!.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"),
            "a late return still finishes it",
        )
        core.push("signIn", """{"phase":"new","ticket":"T7"}""")
        core.waitFor("new") { s.step == Step.New }
    }

    @Test
    fun `Use a passkey opens the account its passkey belongs to, and an unknown one says so`() {
        platform.get =
            { immediate -> if (immediate) PasskeyResult.None else PasskeyResult.Used("cred", "QUNDT1VOVA", "cHJm") }
        val s = signIn()
        ready(s)
        s.usePasskey()
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertFalse(platform.gets.single().second, "the user asked: the sheet may show")
        assertEquals(
            "cHJm",
            core
                .last("recover")
                .getValue("prf")
                .jsonPrimitive.content,
        )
        core.answers["recover"] = { fail("kitUnknown") }
        val u = signIn()
        ready(u)
        u.usePasskey()
        core.waitFor("said") { u.failure != null }
        assertIs<Step.Start>(u.step)
        assertEquals(AccountCopy.KIT_UNKNOWN_PASSKEY, u.failure!!.text)
        assertEquals(
            listOf(Act.Kit, Act.Scan),
            u.failure!!.actions.map { it.act },
            "no held sign-in to start over from",
        )
    }

    private fun googleSheet(answer: GoogleAnswer) {
        platform.google =
            object : GoogleSheet {
                override suspend fun signIn(
                    nonce: String,
                    passkeyRequest: String?,
                ) = answer
            }
        core.answers["nativeSignInStart"] = { ok("""{"flow":"FLOW","nonce":"n","expiresMs":${Long.MAX_VALUE}}""") }
    }

    @Test
    fun `a passkey no account knows, picked in Google's sheet, offers a kit or the QR, and the QR join is in`() {
        googleSheet(GoogleAnswer.Passkey(PasskeyResult.Used("cred", "T1RIRVI", "cHJm")))
        core.answers["recover"] = { fail("kitUnknown") }
        val s = signIn(nativeGoogle = true)
        ready(s)
        s.choose("google")
        core.waitFor("said") { s.failure != null }
        assertIs<Step.Start>(s.step, "no sign-in held: nothing to join or start over with")
        assertEquals(AccountCopy.KIT_UNKNOWN_PASSKEY, s.failure!!.text)
        assertEquals(listOf(Act.Kit, Act.Scan), s.failure!!.actions.map { it.act })
        // Scan from a signed-in device joined it: in, with nothing to link.
        s.joined()
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertFalse("linkIdentity" in core.ops())
    }

    @Test
    fun `no Google account on the phone signs in through the browser`() {
        googleSheet(GoogleAnswer.NoCredential)
        val s = signIn(nativeGoogle = true)
        ready(s)
        s.choose("google")
        core.waitFor("the brokered run") { s.run != null }
        assertEquals(Step.Working("google"), s.step)
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/v1/auth/google/go","expiresMs":9}""")
        core.waitFor("the page opened") { platform.opened.isNotEmpty() }
        assertNull(s.failure)
    }

    @Test
    fun `a failed link after the QR join keeps the join, and a sign-in held elsewhere waits for Account`() {
        val s = signIn()
        ready(s)
        broker(s, """{"phase":"new","ticket":"T8"}""")
        core.waitFor("new") { s.step == Step.New }
        sync.join(ScriptedCore.record()).let { j -> kotlinx.coroutines.runBlocking { j.join() } }
        core.answers["linkIdentity"] = { fail("linkedElsewhere") }
        s.joined()
        core.waitFor("in") { s.step == Step.Done(created = false) }
        assertEquals(SyncAccount.HeldLink("mock", "T8"), sync.heldLink)
        val b = signIn()
        ready(b)
        broker(b, """{"phase":"new","ticket":"T9"}""")
        core.waitFor("new") { b.step == Step.New }
        sync.heldLink = null
        core.answers["linkIdentity"] = { fail("busy") }
        b.joined()
        core.waitFor("in") { b.step == Step.Done(created = false) }
        assertNull(sync.heldLink, "anything else is Account's Link another")
    }

    @Test
    fun `a stray link the core ignores does not hide Didn't finish signing in, nor reach an Auth Tab run`() {
        val s = signIn()
        ready(s)
        s.choose("mock")
        core.waitFor("view") { s.run != null }
        core.push("signIn", """{"phase":"browser","url":"https://sync.example.org/go","expiresMs":9}""")
        core.waitFor("opened") { platform.opened.isNotEmpty() }
        val run = s.run!!
        assertTrue(run.linkReturned("momotus://auth#flow=other&code=c&relay=sync.example.org"))
        s.resumed()
        core.waitFor("asked", timeoutMs = 4_000) { s.failure != null }
        assertEquals(AccountCopy.NO_REDIRECT, s.failure!!.text, "the core kept waiting: no return was taken")
        run.viaAuthTab = true
        assertFalse(
            run.linkReturned("momotus://auth#flow=f&code=c&relay=sync.example.org"),
            "an Auth Tab run takes its return from the tab only",
        )
        assertTrue(run.returned("momotus://auth#flow=f&code=c&relay=sync.example.org"))
    }
}
