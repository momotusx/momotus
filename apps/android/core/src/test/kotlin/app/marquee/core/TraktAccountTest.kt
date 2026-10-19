package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.model.KeystoreRecord
import app.marquee.core.model.TraktStatus
import app.marquee.core.session.CoreOps
import app.marquee.core.trakt.TraktAccount
import app.marquee.core.trakt.TraktRecords
import kotlinx.coroutines.CompletableDeferred
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
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The Trakt record's path: keystore to core at each begin, offered records stored before the ack, sign-out. */
class TraktAccountTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** What happened, in order: core requests (`setTrakt <record>`), keystore writes, view opens and closes. */
    private val events = CopyOnWriteArrayList<String>()

    /** Per op: the answer (`{"ok":...}` fields) to send, possibly after a wait. */
    private val answers = ConcurrentHashMap<String, suspend (JsonObject) -> String>()

    private inner class Records : TraktRecords {
        val held = ConcurrentHashMap<String, KeystoreRecord>()
        var durable = true

        /** False: the file cannot be written, so a delete keeps the record. */
        var deletable = true

        /** Holds a save until completed: the worker stops right before its check and send. */
        @Volatile
        var saving: CompletableDeferred<Unit>? = null

        override fun load(profile: String) = held[profile]

        override fun save(
            profile: String,
            record: KeystoreRecord,
        ): Boolean {
            saving?.let { runBlocking { it.await() } }
            held[profile] = record
            events += "store $profile ${name(record)}"
            return durable
        }

        override fun delete(profile: String): Boolean {
            if (!deletable) {
                events += "delete $profile refused"
                return false
            }
            held.remove(profile)
            events += "delete $profile"
            return true
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
            // Another profile with nothing stored: nothing is sent, the core keeps what it holds.
            events.clear()
            sync.end()
            begin(sync, "p2")
            until("p2 open") { "open p2" in events }
            assertEquals(listOf("close p1", "open p2"), events.toList())
        }

    @Test
    fun `two begins in a row leave one view open`() =
        runBlocking {
            val sync = account(Records())
            begin(sync, "p1")
            begin(sync, "p1")
            until("open") { "open p1" in events }
            delay(150)
            val opened = events.count { it == "open p1" } - events.count { it == "close p1" }
            assertEquals(1, opened, events.toString())
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
            // The file cannot drop it: not signed out here yet, and the record stays for Try again.
            records.deletable = false
            assertFalse(sync.signOut())
            assertEquals(record("r1"), records.held["p1"])
            records.deletable = true
            assertTrue(sync.signOut())
            assertNull(records.held["p1"])
            val order = events.filter { it == "traktSignOut" || it.startsWith("delete") }
            assertEquals(
                listOf("traktSignOut", "traktSignOut", "delete p1 refused", "traktSignOut", "delete p1"),
                order,
            )
        }

    @Test
    fun `a deleted profile's record goes, from this device's delete and from the core's gone push`() =
        runBlocking {
            val records =
                Records().apply {
                    held["p2"] = record("r2")
                    held["p3"] = record("r3")
                }
            val sync = account(records)
            // Deleted here: the page calls forget after deleteProfile.
            sync.forget("p2")
            until("p2 gone") { records.held["p2"] == null }
            // Deleted on another device (or before this start): the core says so.
            client.onProfileGone = sync::forget
            client.onMessage("""{"profile":"gone","id":"p3"}""")
            until("p3 gone") { records.held["p3"] == null }
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

    @Test
    fun `a record whose profile left before its send is never sent, so another profile never gets it`() =
        runBlocking {
            val records = Records()
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            events.clear()
            records.saving = CompletableDeferred()
            views.getValue("p1")(status(record("rA")))
            until("the worker holds the lock") { records.saving != null && events.isEmpty() }
            // The switch to p2: the gate shuts (end), then waits for the worker before switchProfile.
            delay(50)
            sync.end()
            val switched = scope.launch { sync.quiet().also { events += "switch" } }
            records.saving?.complete(Unit)
            switched.join()
            assertEquals(listOf("store p1 rA", "switch"), events.filterNot { it.startsWith("close") })
        }

    @Test
    fun `a switch waits for a send already under way, so the record reaches its own profile first`() =
        runBlocking {
            val records = Records()
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            val reply = CompletableDeferred<Unit>()
            answers["setTrakt"] = { _ ->
                reply.await()
                """"ok":true,"result":{}"""
            }
            events.clear()
            views.getValue("p1")(status(record("rA")))
            until("sent") { "setTrakt rA" in events }
            sync.end()
            val switched = scope.launch { sync.quiet().also { events += "switch" } }
            delay(150)
            assertTrue("switch" !in events, "the switch waited")
            reply.complete(Unit)
            switched.join()
            assertEquals(listOf("store p1 rA", "setTrakt rA", "switch"), events.filterNot { it.startsWith("close") })
        }

    @Test
    fun `the storage warning belongs to the profile whose record it is`() =
        runBlocking {
            val records = Records().apply { durable = false }
            val sync = account(records)
            begin(sync, "p1")
            until("open") { "open p1" in events }
            views.getValue("p1")(status(record("r1")))
            until("p1 says so") { sync.memoryOnly }
            sync.end()
            begin(sync, "p2")
            until("p2 open") { "open p2" in events }
            until("p2 clear") { !sync.memoryOnly }
            // A late save of p1's next record while p2 runs says nothing on p2's page.
            views.getValue("p1")(status(record("r1b")))
            until("stored") { "store p1 r1b" in events }
            delay(50)
            assertFalse(sync.memoryOnly)
            sync.end()
            begin(sync, "p1")
            until("p1 again") { events.count { it == "open p1" } == 2 }
            until("p1 says so again") { sync.memoryOnly }
        }
}
