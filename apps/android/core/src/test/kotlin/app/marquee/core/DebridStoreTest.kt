package app.marquee.core

import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.RestoreAnswer
import app.marquee.core.secrets.DebridCore
import app.marquee.core.secrets.DebridKeystore
import app.marquee.core.secrets.DebridStore
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
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
                    keystore.failing[text("call")] = step.getValue("times").jsonPrimitive.int
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

    private companion object {
        fun id(r: DebridRecord) = "${r.service} ${r.generation} ${r.revision}"

        fun stone(t: DebridTombstone) = "${t.service} ${t.generation}"
    }
}
