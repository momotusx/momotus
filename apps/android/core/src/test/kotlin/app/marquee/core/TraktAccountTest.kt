package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.model.KeystoreRecord
import app.marquee.core.model.TraktStatus
import app.marquee.core.session.CoreOps
import app.marquee.core.trakt.TraktAccount
import app.marquee.core.trakt.TraktRecords
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonElement
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
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The Trakt record's path: keystore to core at each begin, offered records stored before the ack, sign-out. */
class TraktAccountTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** What happened, in order: core requests (`setTrakt <record>`), keystore writes, view opens and closes. */
    private val events = CopyOnWriteArrayList<String>()

    /** Per op: the answer (`{"ok":...}` fields) to send. */
    private val answers = ConcurrentHashMap<String, (JsonObject) -> String>()

    private inner class Records : TraktRecords {
        val held = ConcurrentHashMap<String, KeystoreRecord>()
        var durable = true

        override fun load(profile: String) = held[profile]

        override fun save(
            profile: String,
            record: KeystoreRecord,
        ): Boolean {
            held[profile] = record
            events += "store $profile ${name(record)}"
            return durable
        }

        override fun delete(profile: String) {
            held.remove(profile)
            events += "delete $profile"
        }
    }

    /** The open views' push callbacks, by profile. */
    private val views = ConcurrentHashMap<String, (TraktStatus) -> Unit>()
    private var opening: String? = null

    init {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val op = msg.getValue("op").jsonPrimitive.content
            events += if (op == "setTrakt") "setTrakt ${name(msg["record"])}" else op
            val answer = answers[op] ?: { _ -> """"ok":true,"result":{}""" }
            scope.launch { client.onMessage("""{"id":$id,${answer(msg)}}""") }
        }
    }

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    private fun name(record: JsonElement?) = (record as? JsonObject)?.get("name")?.jsonPrimitive?.content ?: "none"

    private fun record(name: String) = JsonObject(mapOf("name" to JsonPrimitive(name)))

    private fun status(record: KeystoreRecord? = null) =
        TraktStatus(state = "connected", user = "tester", pendingWrites = 0, record = record)

    private fun account(records: Records) =
        TraktAccount(CoreOps(client), records, scope) { push ->
            val profile = opening!!
            views[profile] = push
            events += "open $profile"
            AutoCloseable { events += "close $profile" }
        }

    private fun begin(
        sync: TraktAccount,
        profile: String,
    ) {
        opening = profile
        sync.begin(profile)
    }

    private suspend fun until(
        what: String,
        check: () -> Boolean,
    ) {
        withTimeout(5_000) { while (!check()) delay(5) }
        assertTrue(check(), what)
    }

    @Test
    fun `each begin hands the core the profile's record before its view opens`() =
        runBlocking {
            val records = Records().apply { held["p1"] = record("r1") }
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            assertEquals(listOf("setTrakt r1", "open p1"), events.toList())
            // A lock and its unlock (or a core that came back): sent again, the view opens again.
            sync.end()
            begin(sync, "p1")
            until("open again") { events.count { it == "open p1" } == 2 }
            assertEquals(listOf("setTrakt r1", "open p1", "close p1", "setTrakt r1", "open p1"), events.toList())
            // Another profile: its own record, here none.
            events.clear()
            sync.end()
            begin(sync, "p2")
            until("p2 open") { "open p2" in events }
            assertEquals(listOf("close p1", "setTrakt none", "open p2"), events.toList())
        }

    @Test
    fun `an offered record is stored, then acknowledged with itself, once`() =
        runBlocking {
            val records = Records()
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            events.clear()
            val push = views.getValue("p1")
            push(status(record("r2")))
            // The core offers it again until the ack lands: stored and acknowledged once.
            push(status(record("r2")))
            until("acknowledged") { "setTrakt r2" in events }
            delay(100)
            assertEquals(listOf("store p1 r2", "setTrakt r2"), events.toList())
            assertEquals(record("r2"), records.held["p1"])
            assertEquals("connected", sync.status?.state)
        }

    @Test
    fun `a record offered as its profile leaves is stored for it and handed to no other`() =
        runBlocking {
            val records = Records()
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            val late = views.getValue("p1")
            sync.end()
            begin(sync, "p2")
            until("p2 open") { "open p2" in events }
            events.clear()
            late(status(record("r1b")))
            until("stored") { "store p1 r1b" in events }
            delay(100)
            assertEquals(listOf("store p1 r1b"), events.toList())
            assertNull(records.held["p2"])
            // p1 runs again: the stored record goes over then.
            sync.end()
            begin(sync, "p1")
            until("handed over") { "setTrakt r1b" in events }
        }

    @Test
    fun `signing out deletes the record, and a refusal keeps it`() =
        runBlocking {
            val records = Records().apply { held["p1"] = record("r1") }
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            answers["traktSignOut"] = { _ -> """"ok":false,"error":"the session is locked","code":"locked"""" }
            assertFailsWith<CoreError> { sync.signOut() }
            assertEquals(record("r1"), records.held["p1"])
            answers.remove("traktSignOut")
            sync.signOut()
            assertNull(records.held["p1"])
            val order = events.filter { it == "traktSignOut" || it.startsWith("delete") }
            assertEquals(listOf("traktSignOut", "traktSignOut", "delete p1"), order)
        }

    @Test
    fun `a record kept in memory only says so`() =
        runBlocking {
            val records = Records().apply { durable = false }
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            views.getValue("p1")(status(record("r3")))
            until("acknowledged") { "setTrakt r3" in events }
            assertTrue(sync.memoryOnly)
        }
}
