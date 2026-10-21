package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.SyncRecord
import app.marquee.core.session.CoreOps
import app.marquee.core.sync.RecordStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
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
import java.util.concurrent.atomic.AtomicLong

/**
 * A core that answers by script, for the account flows' tests: each op its answer (`ok(...)` or `fail(...)`),
 * views by name with a snapshot and later pushes, and every request kept in the order the core got it.
 */
class ScriptedCore : AutoCloseable {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    val client = CoreClient(executor.asCoroutineDispatcher())
    val ops = CoreOps(client)

    /** Every request: its op and fields. */
    val asked = CopyOnWriteArrayList<JsonObject>()

    /** Per op: the answer's fields after `"id":`, from the request. */
    val answers = ConcurrentHashMap<String, suspend (JsonObject) -> String>()

    /** Per view: the opening snapshot, as JSON. */
    val views = ConcurrentHashMap<String, (JsonObject) -> String>()

    /** Commands sent to views: the view's name and the command. */
    val commands = CopyOnWriteArrayList<Pair<String, JsonObject>>()

    private val subs = ConcurrentHashMap<Long, String>()

    /** Subscriptions whose reply went to the client: a push after it is applied after it (one queue). */
    private val replied = ConcurrentHashMap.newKeySet<Long>()
    private val nextSub = AtomicLong(100)

    init {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            asked += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull
            val op = msg.getValue("op").jsonPrimitive.content
            when (op) {
                "subscribe" -> {
                    val view = msg.getValue("view").jsonPrimitive.content
                    val opening = views[view]
                    val sub = nextSub.incrementAndGet()
                    subs[sub] = view
                    val params = msg["params"] as? JsonObject ?: JsonObject(emptyMap())
                    val answer =
                        if (opening ==
                            null
                        ) {
                            fail("unknownView")
                        } else {
                            ok("""{"sub":$sub,"snapshot":${opening(params)}}""")
                        }
                    if (id != null) {
                        scope.launch {
                            client.onMessage("""{"id":$id,$answer}""")
                            replied += sub
                        }
                    }
                }

                "command" -> {
                    val sub = msg.getValue("sub").jsonPrimitive.longOrNull
                    commands += (subs[sub] ?: "?") to msg.getValue("command").jsonObject
                    if (id != null) scope.launch { client.onMessage("""{"id":$id,${ok("{}")}}""") }
                }

                else -> {
                    if (id == null) return@attach
                    val answer = answers[op] ?: { _ -> ok("{}") }
                    scope.launch { client.onMessage("""{"id":$id,${answer(msg)}}""") }
                }
            }
        }
    }

    /** Pushes [update] (a snapshot of the same type, as views of records take) to the open view [view]. */
    fun push(
        view: String,
        update: String,
    ) {
        // The newest subscription of [view], once its reply is in: an older one may be closed already.
        waitFor("a subscription to $view") {
            subs.entries
                .filter { it.value == view }
                .maxOfOrNull { it.key }
                ?.let { it in replied } == true
        }
        val sub = subs.entries.filter { it.value == view }.maxOf { it.key }
        client.onMessage("""{"sub":$sub,"updates":[$update]}""")
    }

    /** The ops asked, in order. */
    fun ops(): List<String> = asked.map { it.getValue("op").jsonPrimitive.content }

    /** The last request of [op]. */
    fun last(op: String): JsonObject = asked.last { it.getValue("op").jsonPrimitive.content == op }

    fun waitFor(
        what: String,
        timeoutMs: Long = 15_000,
        check: () -> Boolean,
    ) = runBlocking {
        withTimeout(timeoutMs) {
            while (!check()) delay(5)
        }
    }

    override fun close() {
        client.close()
        executor.shutdownNow()
        scope.cancel()
    }

    companion object {
        fun ok(result: String) = """"ok":true,"result":$result"""

        fun fail(
            code: String?,
            error: String = "nope",
        ) = """"ok":false,"error":"$error"${code?.let { ""","code":"$it"""" } ?: ""}"""

        fun record(revision: Int = 1): SyncRecord = JsonObject(mapOf("revision" to JsonPrimitive(revision)))
    }
}

/** A record store in memory. */
class MemoryStore(
    var held: SyncRecord? = null,
) : RecordStore {
    override fun load() = held

    override fun save(record: SyncRecord?): Boolean {
        held = record
        return true
    }
}
