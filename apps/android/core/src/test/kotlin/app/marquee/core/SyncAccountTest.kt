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
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
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

    private fun report(record: SyncRecord? = null) =
        """{"pushed":0,"stale":0,"refused":0,"waiting":0,"pulled":0,"applied":0,"unreadable":0,"newer":0,"parked":0,
           "oldEpochWrites":0,"clockGapMs":0,"rotationWaitMs":0,"fingerprint":"FP","record":${record ?: "null"},"again":false}"""

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
}
