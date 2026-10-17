package app.marquee.core

import app.marquee.core.model.DebridCredential
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.RestoreAnswer
import app.marquee.core.secrets.DebridCore
import app.marquee.core.secrets.DebridKeystore
import app.marquee.core.secrets.DebridStore
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.coroutines.cancellation.CancellationException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.fail

/** Replays apps/shared/debrid-storage-vectors.json, the cases every app's helper passes. */
@OptIn(ExperimentalCoroutinesApi::class)
class DebridStoreTest {
    private val json = Json { ignoreUnknownKeys = true }

    private class FakeKeystore : DebridKeystore {
        val stored = LinkedHashMap<String, DebridRecord>()
        val stones = mutableListOf<DebridTombstone>()
        val log = mutableListOf<String>()
        val failing = HashMap<String, Int>()
        var holding: String? = null
        var release: CompletableDeferred<Unit>? = null

        private suspend fun gate(call: String) {
            if (holding == call) {
                holding = null
                val wait = CompletableDeferred<Unit>()
                release = wait
                wait.await()
            }
            val left = failing[call] ?: 0
            if (left > 0) {
                failing[call] = left - 1
                log += "$call failed"
                error(call)
            }
        }

        override suspend fun records() = stored.values.toList()

        override suspend fun tombstones() = stones.toList()

        override suspend fun writeRecord(record: DebridRecord) {
            gate("writeRecord")
            stored[record.service] = record
            log += "writeRecord ${record.generation} ${record.revision}"
        }

        override suspend fun deleteRecord(
            service: String,
            generation: String,
        ) {
            gate("deleteRecord")
            if (stored[service]?.generation == generation) stored.remove(service)
            log += "deleteRecord $generation"
        }

        override suspend fun writeTombstone(tombstone: DebridTombstone) {
            gate("writeTombstone")
            stones += tombstone
            log += "writeTombstone ${tombstone.generation}"
        }

        override suspend fun deleteTombstone(tombstone: DebridTombstone) {
            gate("deleteTombstone")
            stones.removeAll { it.generation == tombstone.generation }
            log += "deleteTombstone ${tombstone.generation}"
        }
    }

    /** Answers `restore` the way the core does. */
    private class FakeCore : DebridCore {
        val acks = mutableListOf<String>()
        var failing = 0
        val disconnects = mutableListOf<String>()
        val restores = mutableListOf<Pair<List<String>, List<String>>>()

        override suspend fun restore(
            records: List<DebridRecord>,
            tombstones: List<DebridTombstone>,
        ): RestoreAnswer {
            restores += records.map(::id) to tombstones.map(::stone)
            val gone = tombstones.map { it.generation }.toSet()
            val handed = records.map { it.generation }.toSet()
            return RestoreAnswer(
                refused = records.filter { it.generation in gone }.map { it.generation },
                tombstones = tombstones.filter { it.generation !in handed },
            )
        }

        override suspend fun ack(
            service: String,
            generation: String,
            revision: Long,
        ) {
            acks += "$service $generation $revision"
        }

        override suspend fun disconnect(
            service: String,
            generation: String,
        ) {
            if (failing > 0) {
                failing--
                error("disconnect")
            }
            disconnects += "$service $generation"
        }
    }

    private fun records(value: kotlinx.serialization.json.JsonElement?) =
        (value as JsonArray).map { json.decodeFromJsonElement(DebridRecord.serializer(), it) }

    private fun strings(value: kotlinx.serialization.json.JsonElement?) =
        value!!.jsonArray.map { it.jsonPrimitive.content }

    private suspend fun TestScope.replay(case: JsonObject) {
        val keystore = FakeKeystore()
        val start = case.getValue("keystore").jsonObject
        records(start["records"]).forEach { keystore.stored[it.service] = it }
        start["tombstones"]!!.jsonArray.forEach {
            keystore.stones += json.decodeFromJsonElement(DebridTombstone.serializer(), it)
        }
        val core = FakeCore()
        val errors = mutableListOf<String>()
        var store = DebridStore(keystore, core, this)
        for (step in case.getValue("steps").jsonArray.map { it.jsonObject }) {
            val text = { key: String -> step.getValue(key).jsonPrimitive.content }
            when (text("op")) {
                "pending" -> {
                    store.pending(records(step["records"]))
                }

                "disconnect" -> {
                    val name = "${text("service")} ${text("generation")}"
                    store
                        .disconnect(text("service"), text("generation"))
                        .invokeOnCompletion { if (it != null) errors += name }
                }

                "hold" -> {
                    keystore.holding = text("call")
                }

                "release" -> {
                    keystore.release!!.complete(Unit)
                }

                "fail" -> {
                    val times = step.getValue("times").jsonPrimitive.int
                    if (text("call") == "disconnect") core.failing = times else keystore.failing[text("call")] = times
                }

                "settle" -> {
                    advanceUntilIdle()
                    store.idle()
                }

                "restart" -> {
                    store = DebridStore(keystore, core, this)
                    store.start()
                }

                "expect" -> {
                    step["log"]?.let { assertEquals(strings(it), keystore.log, "log") }
                    step["records"]?.let { assertEquals(strings(it), keystore.stored.values.map(::id), "records") }
                    step["tombstones"]?.let { assertEquals(strings(it), keystore.stones.map(::stone), "tombstones") }
                    step["acks"]?.let { assertEquals(strings(it), core.acks, "acks") }
                    step["disconnects"]?.let { assertEquals(strings(it), core.disconnects, "disconnects") }
                    step["errors"]?.let { assertEquals(strings(it), errors, "errors") }
                    step["restores"]?.let { want ->
                        val expected =
                            want.jsonArray.map {
                                strings(it.jsonObject["records"]) to strings(it.jsonObject["tombstones"])
                            }
                        assertEquals(expected, core.restores, "restores")
                    }
                }

                else -> {
                    fail("unknown op ${text("op")}")
                }
            }
            // Time passes between steps: a queued task starts.
            advanceUntilIdle()
        }
    }

    @Test
    fun everySharedCasePasses() {
        val file = File("../../shared/debrid-storage-vectors.json")
        val cases =
            json
                .parseToJsonElement(file.readText())
                .jsonObject
                .getValue("cases")
                .jsonArray
        for (case in cases.map { it.jsonObject }) {
            val name = case.getValue("name").jsonPrimitive.content
            try {
                runTest { replay(case) }
            } catch (e: AssertionError) {
                throw AssertionError("$name: ${e.message}", e)
            }
        }
    }

    /** A thread-safe keystore and core for the multithreaded cases. */
    private class Shared :
        DebridKeystore,
        DebridCore {
        val stored = java.util.concurrent.ConcurrentHashMap<String, DebridRecord>()
        val stones = java.util.concurrent.CopyOnWriteArrayList<DebridTombstone>()

        @Volatile var cancelWrites = false

        /** When set, a record write waits for it and then throws cancellation. */
        @Volatile var writeGate: CompletableDeferred<Unit>? = null

        @Volatile var writeWaiting = false

        /** The next this many record deletes throw cancellation. */
        @Volatile var cancelDeletes = 0

        /** The next this many core disconnects fail. */
        @Volatile var failDisconnects = 0

        override suspend fun records() = stored.values.toList()

        override suspend fun tombstones() = stones.toList()

        override suspend fun writeRecord(record: DebridRecord) {
            if (cancelWrites) throw CancellationException("scope going")
            writeGate?.let {
                writeWaiting = true
                it.await()
                throw CancellationException("scope going")
            }
            stored[record.service] = record
        }

        override suspend fun deleteRecord(
            service: String,
            generation: String,
        ) {
            if (cancelDeletes > 0) {
                cancelDeletes--
                throw CancellationException("going")
            }
            stored.computeIfPresent(service) { _, r -> r.takeIf { it.generation != generation } }
        }

        override suspend fun writeTombstone(tombstone: DebridTombstone) {
            stones += tombstone
        }

        override suspend fun deleteTombstone(tombstone: DebridTombstone) {
            stones.removeIf { it.generation == tombstone.generation }
        }

        override suspend fun restore(
            records: List<DebridRecord>,
            tombstones: List<DebridTombstone>,
        ) = RestoreAnswer(emptyList(), emptyList())

        override suspend fun ack(
            service: String,
            generation: String,
            revision: Long,
        ) = Unit

        override suspend fun disconnect(
            service: String,
            generation: String,
        ) {
            if (failDisconnects > 0) {
                failDisconnects--
                error("disconnect")
            }
        }
    }

    private fun record(
        generation: String,
        revision: Long,
    ) = DebridRecord("torbox", generation, revision, DebridCredential.Key("k$revision"))

    @Test
    fun aPushRacingADisconnectNeverOutlivesIt() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            repeat(300) { round ->
                val generation = "g$round"
                val shared = Shared()
                shared.stored["torbox"] = record(generation, 1)
                val store = DebridStore(shared, shared, scope)
                val go = CompletableDeferred<Unit>()
                val push =
                    scope.launch {
                        go.await()
                        store.pending(listOf(record(generation, 2)))
                    }
                val drop =
                    scope.launch {
                        go.await()
                        store.disconnect("torbox", generation).await()
                    }
                go.complete(Unit)
                push.join()
                drop.join()
                store.idle()
                assertEquals(null, shared.stored["torbox"], "round $round")
                assertEquals(emptyList(), shared.stones.toList(), "round $round")
            }
            scope.cancel()
        }

    @Test
    fun aCancelledWriteLeavesTheQueueUsable() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            val shared = Shared()
            val store = DebridStore(shared, shared, scope)
            shared.cancelWrites = true
            store.pending(listOf(record("g1", 1)))
            withTimeout(5_000) { store.idle() }
            shared.cancelWrites = false
            store.pending(listOf(record("g1", 2)))
            withTimeout(5_000) { store.idle() }
            assertEquals(2L, shared.stored["torbox"]?.revision)
            scope.cancel()
        }

    @Test
    fun aCancelledScopeSettlesEverything() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default).also { it.cancel() }
            val shared = Shared()
            shared.stored["torbox"] = record("g1", 1)
            val store = DebridStore(shared, shared, scope)
            store.pending(listOf(record("g1", 2)))
            val drop = store.disconnect("torbox", "g1")
            withTimeout(5_000) { store.idle() }
            assertTrue(drop.isCancelled)
            assertEquals(1L, shared.stored["torbox"]?.revision)
            assertEquals(emptyList(), shared.stones.toList())
        }

    @Test
    fun aDisconnectDroppedByCancellationLeavesItsGenerationWritable() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            val shared = Shared()
            val gate = CompletableDeferred<Unit>()
            shared.writeGate = gate
            val store = DebridStore(shared, shared, scope)
            store.pending(listOf(record("g1", 1)))
            withTimeout(5_000) { while (!shared.writeWaiting) delay(5) }
            // Queued behind the held write, then thrown away when that write is cancelled.
            val drop = store.disconnect("torbox", "g1")
            shared.writeGate = null
            gate.complete(Unit)
            withTimeout(5_000) { store.idle() }
            assertTrue(drop.isCancelled)
            store.pending(listOf(record("g1", 2)))
            withTimeout(5_000) { store.idle() }
            assertEquals(2L, shared.stored["torbox"]?.revision, "the generation is still written")
            assertEquals(emptyList(), shared.stones.toList())
            scope.cancel()
        }

    @Test
    fun aCancelledCleanupIsTriedAgain() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            val shared = Shared()
            shared.stored["torbox"] = record("g1", 1)
            shared.cancelDeletes = 1
            val store = DebridStore(shared, shared, scope)
            val drop = store.disconnect("torbox", "g1")
            withTimeout(5_000) { store.idle() }
            assertTrue(drop.isCancelled)
            assertEquals(1, shared.stones.size, "the tombstone stays until the record is gone")
            store.pending(emptyList())
            withTimeout(5_000) { store.idle() }
            assertEquals(null, shared.stored["torbox"])
            assertEquals(emptyList(), shared.stones.toList())
            scope.cancel()
        }

    @Test
    fun aDroppedRepeatDisconnectKeepsAStoredTombstonesGenerationGone() =
        runBlocking {
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            val shared = Shared()
            shared.stored["torbox"] = record("g1", 1)
            // The disconnect and its retry both fail at the core: the tombstone stays stored.
            shared.failDisconnects = 2
            val store = DebridStore(shared, shared, scope)
            runCatching { store.disconnect("torbox", "g1").await() }
            withTimeout(5_000) { store.idle() }
            val gate = CompletableDeferred<Unit>()
            shared.writeGate = gate
            store.pending(listOf(record("g2", 1)))
            withTimeout(5_000) { while (!shared.writeWaiting) delay(5) }
            // Queued behind the held write, then thrown away when that write is cancelled.
            val repeat = store.disconnect("torbox", "g1")
            shared.writeGate = null
            gate.complete(Unit)
            withTimeout(5_000) { store.idle() }
            assertTrue(repeat.isCancelled)
            store.pending(listOf(record("g1", 2)))
            withTimeout(5_000) { store.idle() }
            assertEquals(null, shared.stored["torbox"], "g1 is never written again")
            assertEquals(emptyList(), shared.stones.toList())
            scope.cancel()
        }

    private companion object {
        fun id(r: DebridRecord) = "${r.service} ${r.generation} ${r.revision}"

        fun stone(t: DebridTombstone) = "${t.service} ${t.generation}"
    }
}
