package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.model.SyncRecord
import app.marquee.core.session.CoreOps
import app.marquee.core.sync.RecordStore
import app.marquee.core.sync.SyncAccount
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The account's rules against a scripted core: turns, revisions, `dropped`, joins, storage. */
class SyncAccountTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** Requests in the order the core got them. */
    private val asked = CopyOnWriteArrayList<String>()

    /** Per op: the answer (`{"ok":...}` fields) to send, possibly after a wait. */
    private val answers = ConcurrentHashMap<String, suspend (JsonObject) -> String>()

    private class Store(
        var held: SyncRecord?,
        var durable: Boolean = true,
    ) : RecordStore {
        val writes = CopyOnWriteArrayList<SyncRecord?>()

        override fun load() = held

        override fun save(record: SyncRecord?): Boolean {
            held = record
            writes += record
            return durable
        }
    }

    init {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val op = msg.getValue("op").jsonPrimitive.content
            asked += op
            val answer = answers[op] ?: { _ -> ok("{}") }
            scope.launch { client.onMessage("""{"id":$id,${answer(msg)}}""") }
        }
    }

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    private fun ok(result: String) = """"ok":true,"result":$result"""

    private fun fail(
        code: String?,
        error: String = "nope",
    ) = """"ok":false,"error":"$error"${code?.let { ""","code":"$it"""" } ?: ""}"""

    private fun record(
        revision: Int,
        account: String = "a",
    ): SyncRecord = JsonObject(mapOf("revision" to JsonPrimitive(revision), "account" to JsonPrimitive(account)))

    private fun SyncRecord?.revision() = (this?.get("revision") as? JsonPrimitive)?.content?.toInt()

    private fun SyncRecord?.account() = (this?.get("account") as? JsonPrimitive)?.content

    private val devices = """{"members":[],"fingerprint":"FP","epoch":1,"rotationPending":false}"""

    private fun report(
        record: SyncRecord? = null,
        refused: Int = 0,
        fingerprint: String = "FP",
    ) = """{"pushed":0,"stale":0,"refused":$refused,"waiting":0,"pulled":0,"applied":0,"unreadable":0,"newer":0,
           "parked":0,"oldEpochWrites":0,"clockGapMs":0,"rotationWaitMs":0,"fingerprint":"$fingerprint",
           "record":${record ?: "null"},"again":false}"""

    private fun member(
        id: String,
        kit: Boolean = false,
        me: Boolean = false,
        sponsor: String? = null,
    ) = """{"id":"$id","kit":$kit,"name":"$id","sponsor":${sponsor?.let {
        "\"$it\""
    } ?: "null"},"addedMs":1,"thisDevice":$me}"""

    private val me = member("me", me = true)

    /** A kit [maker] made. */
    private fun kit(
        id: String,
        maker: String = "me",
    ) = member(id, kit = true, sponsor = maker)

    /** The lists the core answers in turn; the last one again after that. */
    private fun lists(vararg answers: String) {
        var read = 0
        this.answers["syncDevices"] = { ok(answers[minOf(read++, answers.lastIndex)]) }
    }

    private fun list(
        vararg members: String,
        fingerprint: String = "FP",
    ) = """{"members":[${members.joinToString(",")}],"fingerprint":"$fingerprint","epoch":1,"rotationPending":false}"""

    private fun account(store: Store) = SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO)

    private suspend fun until(
        what: String,
        check: () -> Boolean,
    ) {
        withTimeout(5_000) { while (!check()) delay(5) }
        assertTrue(check(), what)
    }

    @Test
    fun `a record in an answer is kept only when it is not older than the stored one`() =
        runBlocking {
            val store = Store(record(5))
            val sync = account(store)
            answers["syncDevices"] = { ok(devices.replace("}", ""","record":${record(4)}}""")) }
            sync.loadDevices()
            assertEquals(5, store.held.revision())
            answers["approvePairing"] = { ok("""{"code":"123456","record":${record(7)}}""") }
            assertEquals("123456", sync.approvePairing("marquee:pair?v=2&s=https%3A%2F%2Fx&p=1"))
            assertEquals(7, store.held.revision())
        }

    @Test
    fun `ops on the record run one at a time, in the order asked`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            val first = CompletableDeferred<Unit>()
            answers["syncDevices"] = {
                first.await()
                ok(devices)
            }
            answers["lostPreview"] = { ok("""{"devices":[],"kits":[]}""") }
            val a = async { sync.loadDevices() }
            until("the first op reached the core") { asked.contains("syncDevices") }
            val b = async { sync.lostPreview("x") }
            delay(200)
            // The second waits for the first's answer.
            assertFalse(asked.contains("lostPreview"))
            first.complete(Unit)
            a.await()
            b.await()
            assertEquals(listOf("syncDevices", "lostPreview"), asked.toList())
        }

    @Test
    fun `remove and lost stay done when the list after them fails`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            answers["removeDevice"] = { ok(record(2).toString()) }
            answers["reportLost"] = { ok(record(3).toString()) }
            answers["syncDevices"] = { fail(null, "http status 502") }
            sync.removeDevice("tv")
            assertEquals(2, store.held.revision())
            sync.reportLost("phone", listOf("tablet"))
            assertEquals(3, store.held.revision())
        }

    @Test
    fun `dropped forgets the account here, the reason stays in words`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            sync.startRounds()
            answers["syncNow"] = { fail("dropped") }
            sync.request()
            until("forgotten") { !sync.joined }
            assertNull(store.held)
            assertEquals("dropped", sync.problem?.code)
            assertTrue(sync.problem!!.words.startsWith("This device was removed"))
        }

    @Test
    fun `an answer from before a join is not stored over the new account`() =
        runBlocking {
            val store = Store(record(9, account = "old"))
            val sync = account(store)
            val held = CompletableDeferred<Unit>()
            answers["syncDevices"] = {
                held.await()
                ok(devices.replace("}", ""","record":${record(10, account = "old")}}"""))
            }
            val old = async { runCatching { sync.loadDevices() } }
            until("the old account's op is in flight") { asked.contains("syncDevices") }
            sync.join(record(1, account = "new")).join()
            held.complete(Unit)
            old.await()
            assertEquals("new", store.held.account())
            assertEquals(1, store.held.revision())
            // The old list does not show for the new account either.
            assertNull(sync.devices)
        }

    @Test
    fun `a record the keystore holds in memory only pauses sync until it is stored`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            answers["approvePairing"] = { ok("""{"code":"1","record":${record(2)}}""") }
            store.durable = false
            assertFailsWith<CoreError> { sync.approvePairing("marquee:pair?v=2&s=https%3A%2F%2Fx&p=1") }
            assertEquals("storage", sync.problem?.code)
            // Nothing goes out while the record is not stored.
            val before = asked.size
            assertFailsWith<CoreError> { sync.lostPreview("x") }
            assertEquals(before, asked.size)
            // Once the keystore takes it again, ops go on with it.
            store.durable = true
            answers["lostPreview"] = { ok("""{"devices":["t"],"kits":[]}""") }
            assertEquals(listOf("t"), sync.lostPreview("x").devices)
            assertEquals(2, store.held.revision())
        }

    @Test
    fun `a kit join asks for a new kit until one is made, and leaving forgets the record`() =
        runBlocking {
            val store = Store(null)
            val saved = CopyOnWriteArrayList<Boolean>()
            val sync = SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, saveRecovered = { saved += it })
            answers["recover"] = { ok(record(1).toString()) }
            sync.recover("https://sync.example.org", "KIT", "Phone")
            assertTrue(sync.joined)
            assertTrue(sync.recovered)
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            assertEquals("ABCD", sync.createRecoveryKit())
            assertFalse(sync.recovered)
            assertEquals(listOf(true, false), saved.toList())
            sync.signOut(force = false)
            assertFalse(sync.joined)
            assertNull(store.held)
        }

    @Test
    fun `unsent edits keep the account and say why`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            answers["signOut"] = { fail("unsent") }
            val e = assertFailsWith<CoreError> { sync.signOut(force = false) }
            assertEquals("unsent", e.code)
            assertTrue(sync.joined)
            assertEquals(1, store.held.revision())
        }

    @Test
    fun `the server's list is taken only as shown, and a list that moved since is refused`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            var sent: JsonObject? = null
            answers["useServerRoster"] = { msg ->
                sent = msg
                ok(record(2).toString())
            }
            sync.useServerRoster(listOf("tv"), "HEAD1")
            assertEquals("HEAD1", sent!!.getValue("head").jsonPrimitive.content)
            assertEquals(listOf("tv"), sent!!.getValue("drop").jsonArray.map { it.jsonPrimitive.content })
            assertEquals(2, store.held.revision())
            answers["useServerRoster"] = { fail("rosterChanged") }
            val e = assertFailsWith<CoreError> { sync.useServerRoster(emptyList(), "HEAD1") }
            assertEquals("rosterChanged", e.code)
            assertEquals(2, store.held.revision())
        }

    @Test
    fun `They differ ends the approval through the core and keeps the new key`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            var qr: String? = null
            answers["cancelPairing"] = { msg ->
                qr = msg.getValue("qr").jsonPrimitive.content
                ok(record(3).toString())
            }
            sync.cancelPairing("marquee:pair?v=2&s=x&p=1")
            assertEquals("marquee:pair?v=2&s=x&p=1", qr)
            assertEquals(3, store.held.revision())
        }

    @Test
    fun `an account started on a page that closed before the answer is still stored`() =
        runBlocking {
            val store = Store(null)
            val sync = account(store)
            val answer = CompletableDeferred<Unit>()
            answers["createSyncAccount"] = {
                answer.await()
                ok(record(1).toString())
            }
            val page = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            page.launch { sync.createAccount("https://sync.example.org", "", "Phone") }
            until("the core was asked") { asked.contains("createSyncAccount") }
            // Back: the page's work ends before the sync server answered.
            page.cancel()
            answer.complete(Unit)
            until("stored") { store.held != null }
            until("joined") { sync.joined }
            assertEquals(1, store.held.revision())
        }

    @Test
    fun `a failed read of the list marks it stale, and the 12 characters follow the newest answer`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            sync.startRounds()
            answers["syncDevices"] = { ok(list(member("me", me = true), fingerprint = "OLD")) }
            sync.loadDevices()
            assertEquals("OLD", sync.fingerprint)
            assertFalse(sync.listFailed)
            // A round sees a change; reading the list after it fails.
            answers["syncDevices"] = { fail(null, "http status 502") }
            answers["syncNow"] = { ok(report(fingerprint = "NEW")) }
            sync.request()
            until("the round's 12 characters") { sync.fingerprint == "NEW" }
            until("the list marked stale") { sync.listFailed }
            assertEquals("OLD", sync.devices?.fingerprint)
            answers["syncDevices"] = { ok(list(member("me", me = true), fingerprint = "NEW")) }
            sync.loadDevices()
            assertFalse(sync.listFailed)
        }

    @Test
    fun `a refused change stays listed after a clean round, until dismissed`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            sync.startRounds()
            answers["syncDevices"] = { ok(devices) }
            answers["syncNow"] = { ok(report(refused = 1)) }
            sync.request()
            until("counted") { sync.failed.refused == 1 }
            answers["syncNow"] = { ok(report()) }
            val before = asked.count { it == "syncNow" }
            sync.request()
            until("a clean round ran") { asked.count { it == "syncNow" } > before && !sync.syncing }
            assertEquals(1, sync.failed.refused)
            sync.dismissFailed()
            assertEquals(0, sync.failed.refused)
        }

    @Test
    fun `an op asked right after a join runs on the new account`() =
        runBlocking {
            val store = Store(record(9, account = "old"))
            val sync = account(store)
            var sentAccount: String? = null
            answers["lostPreview"] = { msg ->
                sentAccount = (msg.getValue("record") as JsonObject).account()
                ok("""{"devices":[],"kits":[]}""")
            }
            sync.join(record(1, account = "new"))
            sync.lostPreview("x")
            assertEquals("new", sentAccount)
        }

    @Test
    fun `stopping here only sends nothing to the server and forgets the account here`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            var local: Boolean? = null
            answers["signOut"] = { msg ->
                local =
                    msg
                        .getValue("local")
                        .jsonPrimitive.content
                        .toBoolean()
                ok("{}")
            }
            sync.leaveHere()
            assertEquals(true, local)
            assertNull(store.held)
            assertFalse(sync.joined)
            assertTrue(sync.leftHere)
            // A plain stop says nothing about the server still listing this device.
            store.held = record(1)
            val other = account(store)
            other.signOut(force = false)
            assertFalse(other.leftHere)
        }

    @Test
    fun `a kit is held until saved, and one that went unseen goes with the next kit`() =
        runBlocking {
            val store = Store(record(1))
            val saved = CopyOnWriteArrayList<List<String>>()
            val sync =
                SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, savePendingKits = { saved += it })
            lists(list(me), list(me, kit("k1")))
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            assertEquals("ABCD", sync.createRecoveryKit())
            assertEquals(SyncAccount.HeldKit("k1", "ABCD"), sync.heldKit)
            // Its id is kept (never the kit), so a run that ends before "I saved it" knows it went unseen.
            assertEquals(listOf("k1"), saved.last())
            sync.kitSaved()
            assertNull(sync.heldKit)
            assertEquals(emptyList(), saved.last())
            // The next run: the app ended before "I saved it".
            val later =
                SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, pendingKitsAtStart = listOf("k1"))
            assertEquals(listOf("k1"), later.unseenKits)
            asked.clear()
            answers["removeDevice"] = { msg ->
                assertEquals("k1", msg.getValue("device").jsonPrimitive.content)
                ok(record(3).toString())
            }
            lists(list(me, kit("k1")), list(me, kit("k2")))
            answers["createRecoveryKit"] = { ok("""{"kit":"EFGH","record":${record(4)}}""") }
            assertEquals("EFGH", later.createRecoveryKit())
            assertTrue(asked.indexOf("removeDevice") in 0 until asked.indexOf("createRecoveryKit"))
            assertEquals(emptyList(), later.unseenKits)
            assertEquals("k2", later.heldKit?.id)
        }

    @Test
    fun `only a kit this device made since the list before is taken as this kit`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            // Another device added a kit between the two reads: not ours.
            lists(list(me, member("tv")), list(me, member("tv"), kit("kTv", maker = "tv"), kit("k1")))
            sync.createRecoveryKit()
            assertEquals("k1", sync.heldKit?.id)
            sync.kitSaved()
            // Only another device's kit came: ours is not in the list yet, so its id stays unknown.
            lists(list(me, member("tv")), list(me, member("tv"), kit("kTv2", maker = "tv")))
            sync.createRecoveryKit()
            assertEquals("", sync.heldKit?.id)
            sync.kitSaved()
            // No list before: nothing can be told apart.
            lists(list(me)) // unused
            answers["syncDevices"] = { fail(null, "http status 502") }
            sync.createRecoveryKit()
            assertEquals("", sync.heldKit?.id)
        }

    @Test
    fun `a held kit removed before it was saved is gone, and so is its waiting state`() =
        runBlocking {
            val store = Store(record(1))
            val saved = CopyOnWriteArrayList<List<String>>()
            val sync =
                SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, savePendingKits = { saved += it })
            lists(list(me), list(me, kit("k1")))
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            sync.createRecoveryKit()
            answers["removeDevice"] = { ok(record(3).toString()) }
            answers["syncDevices"] = { fail(null, "http status 502") }
            // Removed here, while the list cannot be read: the kit is gone all the same.
            sync.removeDevice("k1")
            assertNull(sync.heldKit)
            assertEquals(emptyList(), saved.last())
            // Removed from another device: the next list without it ends the waiting too.
            lists(list(me), list(me, kit("k2")))
            sync.createRecoveryKit()
            assertEquals("k2", sync.heldKit?.id)
            lists(list(me))
            sync.loadDevices()
            assertNull(sync.heldKit)
        }

    @Test
    fun `a kit whose id could not be read is never claimed gone, and a new kit keeps the older ones`() =
        runBlocking {
            val store = Store(record(1))
            val saved = CopyOnWriteArrayList<List<String>>()
            answers["syncDevices"] = { fail(null, "http status 502") }
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            val sync =
                SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, savePendingKits = { saved += it })
            sync.createRecoveryKit()
            assertEquals(listOf(SyncAccount.UNKNOWN), saved.last())
            // The next run, with an older kit that went unseen too: a new kit keeps both pending.
            val later =
                SyncAccount(
                    CoreOps(client),
                    store,
                    scope,
                    io = Dispatchers.IO,
                    pendingKitsAtStart = listOf("k9", SyncAccount.UNKNOWN),
                    savePendingKits = { saved += it },
                )
            assertTrue(later.unknownUnseen)
            assertEquals(listOf("k9"), later.unseenKits)
            answers["createRecoveryKit"] = { ok("""{"kit":"EFGH","record":${record(3)}}""") }
            later.createRecoveryKit()
            // The list could not say, so neither is claimed gone, and the new kit joins them.
            assertTrue(later.unknownUnseen)
            assertEquals(listOf("k9"), later.unseenKits)
            assertEquals(listOf("k9", SyncAccount.UNKNOWN, SyncAccount.UNKNOWN), saved.last())
            // Only the user settles the unknown one, after checking the list; the held kit stays.
            later.dismissUnknownUnseen()
            assertFalse(later.unknownUnseen)
            assertEquals(listOf("k9", SyncAccount.UNKNOWN), saved.last())
        }

    @Test
    fun `the hold waits while the kit page is open, and says it ran out when it does`() =
        runBlocking {
            val store = Store(record(1))
            val sync = SyncAccount(CoreOps(client), store, scope, io = Dispatchers.IO, holdMs = 100)
            lists(list(me), list(me, kit("k1")))
            answers["createRecoveryKit"] = { ok("""{"kit":"ABCD","record":${record(2)}}""") }
            sync.kitPage(open = true)
            sync.createRecoveryKit()
            delay(400)
            // Still on screen: the user may be copying it by hand.
            assertEquals("ABCD", sync.heldKit?.kit)
            // Left, then opened again before the hold ran out: on screen again, so it waits again.
            sync.kitPage(open = false)
            sync.kitPage(open = true)
            delay(400)
            assertEquals("ABCD", sync.heldKit?.kit)
            sync.kitPage(open = false)
            until("let go") { sync.heldKit == null }
            assertTrue(sync.kitExpired)
            assertEquals(listOf("k1"), sync.unseenKits)
        }

    @Test
    fun `a kit made on a page that closed before the answer is still held`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            val answer = CompletableDeferred<Unit>()
            lists(list(me), list(me, kit("k1")))
            answers["createRecoveryKit"] = {
                answer.await()
                ok("""{"kit":"ABCD","record":${record(2)}}""")
            }
            val page = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            page.launch { sync.createRecoveryKit() }
            until("the core was asked") { asked.contains("createRecoveryKit") }
            page.cancel()
            answer.complete(Unit)
            until("held") { sync.heldKit != null }
            assertEquals("ABCD", sync.heldKit?.kit)
            assertEquals(2, store.held.revision())
        }

    @Test
    fun `stopping here on a page that closed while the core answered still forgets the account here`() =
        runBlocking {
            val store = Store(record(1))
            val sync = account(store)
            val answer = CompletableDeferred<Unit>()
            answers["signOut"] = {
                answer.await()
                ok("{}")
            }
            val page = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            page.launch { sync.leaveHere() }
            until("the core was asked") { asked.contains("signOut") }
            page.cancel()
            answer.complete(Unit)
            until("forgotten here") { !sync.joined }
            assertNull(store.held)
            assertTrue(sync.leftHere)
        }

    @Test
    fun `a fork leaves only stopping here, and a round that waits does not`() =
        runBlocking {
            val sync = account(Store(record(1)))
            sync.startRounds()
            answers["syncNow"] = { fail("fork") }
            sync.request()
            until("stuck on the fork") { sync.problem?.code == "fork" }
            assertTrue(sync.stuck)
            answers["syncNow"] = { fail(null, "timed out") }
            sync.request()
            until("a plain failure") { sync.problem?.code == null && sync.problem != null }
            assertFalse(sync.stuck)
            assertTrue(SyncAccount.stuck("notJoined"))
        }

    @Test
    fun `a join from pairing says it is saving at once`() {
        val sync = account(Store(null))
        sync.didJoin(record(1))
        // Before any thread ran the write: Done waits, never shows and goes again.
        assertTrue(sync.joining)
    }
}
