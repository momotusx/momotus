package app.marquee.core.client

import androidx.compose.runtime.snapshots.Snapshot
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import kotlin.time.Duration

/** The Rust core, from the client's side: one request text out. Replies come in through [CoreClient.onMessage]. */
fun interface CoreTransport {
    fun send(text: String)
}

/**
 * The core answered `ok: false` (its plain text is the message, and [code] names the failures an app
 * can act on, such as "dropped" for sync), or did not answer in time.
 */
class CoreError(
    message: String,
    val timedOut: Boolean = false,
    val code: String? = null,
    /** With code `locked`: how long until the next PIN try is allowed. */
    val retryAfterMs: Long? = null,
    /** With code `viewsOpen`: the views still open. */
    val views: List<String> = emptyList(),
) : Exception(message)

/** The protocol's JSON settings: tolerant of keys a newer core adds. Tests decode strictly. */
val CoreJson: Json =
    Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

/** What a live view does with what the core sends. Called on the state thread inside one Compose snapshot. */
interface ViewSink<S, U> {
    fun onSnapshot(snapshot: S)

    fun onUpdates(updates: List<U>)

    /**
     * Decoding or applying failed. A reason starting with "desync" means the view's list can no
     * longer be trusted and it should subscribe again. Called on the state thread.
     */
    fun onFailure(reason: String)
}

class ViewBinding<S, U>(
    val snapshot: KSerializer<S>,
    val update: KSerializer<U>,
    val sink: ViewSink<S, U>,
)

/** What the client queues when its inbox overflows: handled as the core's own overflow message. */
private const val OVERFLOW = """{"core":"overflow"}"""

private class Raw(
    val text: String,
    val arrivedNs: Long,
    /** Counted against the inbox budget (the overflow marker is not). */
    val counted: Boolean = true,
)

private sealed interface Pending {
    class Plain(
        val deferred: CompletableDeferred<JsonElement>,
    ) : Pending

    class Subscribe(
        val binding: ViewBinding<*, *>,
        val deferred: CompletableDeferred<Long>,
    ) : Pending
}

/** Merged pushes of one subscription within a batch: one apply call per view per batch. */
private class PushGroup<U>(
    val binding: ViewBinding<*, U>,
) {
    val updates = ArrayList<U>()
}

/**
 * Client for the core protocol (plans/core-protocol.md), the Kotlin twin of `bridge.ts`.
 *
 * Threads: [onMessage] is called on a core thread and only enqueues, because the core's
 * forwarding task calls it synchronously on a tokio worker. One state thread (the dispatcher
 * given to the constructor) parses, completes requests, and applies views, so it is the only
 * writer of view state and Compose reads it from the main thread. Everything that reaches the
 * state thread together is applied in one Compose snapshot, so a batch is never seen half done.
 *
 * Replies match requests by id and may arrive in any order (the core runs requests that wait on
 * the network alongside the others).
 */
@OptIn(ExperimentalCoroutinesApi::class)
class CoreClient(
    dispatcher: CoroutineDispatcher,
    val json: Json = CoreJson,
    private val metrics: BridgeMetrics? = null,
    private val nanoTime: () -> Long = System::nanoTime,
    /**
     * Characters the inbox may hold before the state thread takes them (core backlog C26, as the
     * core's own output budget): past it, that core's session ends as an overflow and a new core
     * opens, so an app that stalls never piles up the core's messages without bound.
     */
    private val inboxBudget: Long = 8L * 1024 * 1024,
) : AutoCloseable {
    private val scope = CoroutineScope(SupervisorJob() + dispatcher)
    private val inbox = Channel<Raw>(Channel.UNLIMITED)
    private val queued = AtomicLong(0)

    @Volatile
    private var overflowed = false
    private val nextId = AtomicLong(1)
    private val pending = ConcurrentHashMap<Long, Pending>()
    private val subs = ConcurrentHashMap<Long, ViewBinding<*, *>>()
    private val closed: MutableSet<Long> = ConcurrentHashMap.newKeySet()
    private val early = HashMap<Long, ArrayDeque<String>>()

    // state thread only, JSON arrays of updates
    @Volatile
    private var attached = CompletableDeferred<CoreTransport>()

    /** Counts the cores that died. Subscription ids restart in a new core, so a handle from an older one is dead. */
    @Volatile
    internal var epoch = 0
        private set

    private val orphans = ArrayList<ViewBinding<*, *>>() // state thread only: views the dead core left open

    private var dead = false // state thread only: the core died and no new one is attached yet

    /** Called on the state thread when the core's request loop ended by a panic (plans/core-protocol.md). */
    @Volatile
    var onCoreDied: ((reason: String) -> Unit)? = null

    /** A profile was removed, here or on another device (`{"profile": "gone", id}`): drop its keystore entries. */
    @Volatile
    var onProfileGone: ((id: String) -> Unit)? = null

    init {
        scope.launch { pump() }
    }

    /**
     * Connects the core. Requests made earlier wait for this, outside their timeout. Called again
     * after the core died and was opened anew: the views it left open subscribe again.
     */
    fun attach(transport: CoreTransport) {
        overflowed = false
        if (attached.complete(transport)) return
        attached = CompletableDeferred<CoreTransport>().also { it.complete(transport) }
        scope.launch { revive() }
    }

    /** The core could not open: every waiting and later request fails with this reason. */
    fun attachFailed(reason: String) {
        attached.completeExceptionally(CoreError(reason))
    }

    /** Entry point for the core's listener. Never blocks, never parses. */
    fun onMessage(text: String) {
        // After an overflow, the rest of that core's messages are dropped until a new one attaches.
        if (overflowed) return
        val size = text.length.toLong()
        if (queued.addAndGet(size) > inboxBudget) {
            queued.addAndGet(-size)
            overflowed = true
            inbox.trySend(Raw(OVERFLOW, nanoTime(), counted = false))
            return
        }
        inbox.trySend(Raw(text, nanoTime()))
    }

    /** Runs [block] on the state thread, the only thread that writes view state. */
    fun onState(block: () -> Unit) {
        scope.launch { block() }
    }

    private fun encode(
        id: Long,
        op: String,
        fields: Map<String, JsonElement>,
    ): String =
        buildJsonObject {
            put("id", id)
            put("op", op)
            for ((key, value) in fields) put(key, value)
        }.toString()

    /** One request and its reply. The core's error text becomes [CoreError]. */
    suspend fun request(
        op: String,
        fields: Map<String, JsonElement> = emptyMap(),
        timeout: Duration = CoreTimeouts.forOp(op),
    ): JsonElement {
        val transport = attached.await()
        val id = nextId.getAndIncrement()
        val deferred = CompletableDeferred<JsonElement>()
        pending[id] = Pending.Plain(deferred)
        try {
            transport.send(encode(id, op, fields))
            return withTimeout(timeout) { deferred.await() }
        } catch (e: TimeoutCancellationException) {
            throw CoreError("$op did not answer in $timeout", timedOut = true)
        } finally {
            pending.remove(id)
        }
    }

    /** Fire and forget, for commands sent from click handlers. Failures are logged. */
    fun post(
        op: String,
        fields: Map<String, JsonElement> = emptyMap(),
    ) {
        scope.launch {
            try {
                request(op, fields)
            } catch (e: Exception) {
                CoreLog.w("$op failed: ${CoreLog.describe(e)}")
            }
        }
    }

    suspend fun <S, U> subscribe(
        view: String,
        params: JsonObject,
        binding: ViewBinding<S, U>,
    ): Subscription {
        val transport =
            try {
                attached.await()
            } catch (e: CoreError) {
                // The core is down: this view subscribes again when a new one is attached.
                scope.launch { orphans.add(binding) }
                throw e
            }
        val born = epoch
        val id = nextId.getAndIncrement()
        val deferred = CompletableDeferred<Long>()
        pending[id] = Pending.Subscribe(binding, deferred)
        var handedOver = false
        try {
            transport.send(encode(id, "subscribe", mapOf("view" to JsonPrimitive(view), "params" to params)))
            val sub = withTimeout(CoreTimeouts.local) { deferred.await() }
            handedOver = true
            return Subscription(this, sub, born)
        } catch (e: TimeoutCancellationException) {
            throw CoreError("subscribe $view did not answer in ${CoreTimeouts.local}", timedOut = true)
        } finally {
            pending.remove(id)
            // The reply was handled but nobody took the subscription (cancelled or timed out
            // meanwhile): close it, or the core keeps the view alive for the whole process.
            if (!handedOver && deferred.isCompleted && !deferred.isCancelled) unsubscribe(deferred.getCompleted(), born)
        }
    }

    internal fun unsubscribe(
        sub: Long,
        born: Int,
    ) {
        if (born != epoch) return // that core is gone, and its ids now mean other views
        closed.add(sub)
        subs.remove(sub)
        post("unsubscribe", mapOf("sub" to JsonPrimitive(sub)))
    }

    private suspend fun pump() {
        for (first in inbox) {
            val batch = ArrayList<Raw>()
            batch.add(first)
            while (true) batch.add(inbox.tryReceive().getOrNull() ?: break)
            queued.addAndGet(-batch.sumOf { if (it.counted) it.text.length.toLong() else 0L })
            try {
                process(batch)
            } catch (e: Exception) {
                CoreLog.e("batch failed: ${CoreLog.describe(e)}")
            }
        }
    }

    private fun process(batch: List<Raw>) {
        val started = nanoTime()
        val actions = ArrayList<() -> Unit>()
        val completions = ArrayList<() -> Unit>()
        val groups = HashMap<Long, PushGroup<Any?>>()
        for (raw in batch) {
            try {
                // Pushes are the hot path: read the subscription id off the front of the text and
                // decode the updates straight into typed values, without a JSON tree in between.
                val head = PushHead.scan(raw.text)
                if (head != null) {
                    push(head.sub, raw.text.substring(head.updatesStart, raw.text.length - 1), actions, groups)
                    continue
                }
                val message = json.parseToJsonElement(raw.text).jsonObject
                val sub = message["sub"]?.jsonPrimitive?.longOrNull
                val updates = message["updates"] as? JsonArray
                if (sub != null && updates != null) {
                    push(sub, updates.toString(), actions, groups)
                } else if (message["core"]?.jsonPrimitive?.contentOrNull == "died") {
                    died(message["reason"]?.jsonPrimitive?.contentOrNull ?: "unknown", actions, completions)
                } else if (message["core"]?.jsonPrimitive?.contentOrNull == "overflow") {
                    // The app fell behind the core's output budget (core backlog C20): that session
                    // ended, and a new core opens as after a death.
                    died("overflow: the app fell behind the core's messages", actions, completions)
                } else {
                    reply(message, actions, completions, groups)
                }
            } catch (e: Exception) {
                // One malformed message must not drop the rest of the batch.
                CoreLog.e("cannot handle a message from the core: ${CoreLog.describe(e)}")
            }
        }
        try {
            if (actions.isNotEmpty()) Snapshot.withMutableSnapshot { for (action in actions) action() }
        } catch (e: Exception) {
            CoreLog.e("a batch could not be applied: ${CoreLog.describe(e)}")
            for (binding in subs.values) binding.sink.onFailure("desync: the batch could not be applied")
        } finally {
            // Replies are answered even when applying failed: the core did answer.
            for (completion in completions) completion()
        }
        val done = nanoTime()
        metrics?.recordBatch(done - started)
        if (metrics != null) for (raw in batch) metrics.recordMessage(done - raw.arrivedNs)
    }

    /** The core's request loop ended: fail what waits for it, mark its views, and refuse new requests until it is opened again. */
    private fun died(
        reason: String,
        actions: MutableList<() -> Unit>,
        completions: MutableList<() -> Unit>,
    ) {
        CoreLog.e("the core stopped: ${reason.take(200)}")
        val message = "The core stopped."
        epoch++
        attached = CompletableDeferred<CoreTransport>().also { it.completeExceptionally(CoreError(message)) }
        val waiting = pending.values.toList()
        val open = subs.values.toList()
        subs.clear()
        closed.clear()
        early.clear()
        // Until a new core is attached, whatever still arrives is the dead core's.
        dead = true
        // Views still waiting for their subscribe reply come back too, not only the open ones.
        orphans.addAll(open)
        for (w in waiting) if (w is Pending.Subscribe) orphans.add(w.binding)
        actions.add { for (binding in open) binding.sink.onFailure(message) }
        completions.add {
            for (w in waiting) {
                when (w) {
                    is Pending.Plain -> w.deferred.completeExceptionally(CoreError(message))
                    is Pending.Subscribe -> w.deferred.completeExceptionally(CoreError(message))
                }
            }
            onCoreDied?.invoke(reason)
        }
    }

    /** A new core is attached: views the dead one left open subscribe again, on fresh ids. */
    private fun revive() {
        dead = false
        early.clear()
        val old = orphans.toList()
        orphans.clear()
        Snapshot.withMutableSnapshot {
            for (binding in old) {
                try {
                    binding.sink.onFailure("desync: the core restarted")
                } catch (e: Exception) {
                    CoreLog.e("a view could not come back: ${CoreLog.describe(e)}")
                }
            }
        }
    }

    @Suppress("UNCHECKED_CAST")
    private fun push(
        sub: Long,
        updates: String,
        actions: MutableList<() -> Unit>,
        groups: MutableMap<Long, PushGroup<Any?>>,
    ) {
        if (dead || sub in closed) return
        val binding = subs[sub] as ViewBinding<Any?, Any?>?
        if (binding == null) {
            // A push before its subscribe reply was handled: keep it, briefly and bounded.
            val held = early.getOrPut(sub) { ArrayDeque() }
            if (held.size < EARLY_LIMIT) held.addLast(updates)
            return
        }
        val group =
            groups.getOrPut(sub) {
                PushGroup(binding).also { g ->
                    actions.add { guarded(binding) { binding.sink.onUpdates(g.updates) } }
                }
            }
        try {
            group.updates.addAll(json.decodeFromString(ListSerializer(binding.update), updates))
        } catch (e: Exception) {
            // Updates are diffs against the list the view holds: one lost makes it untrustworthy.
            CoreLog.e("cannot decode an update for sub $sub: ${CoreLog.describe(e)}")
            actions.add { binding.sink.onFailure("desync: an update could not be decoded") }
        }
    }

    @Suppress("UNCHECKED_CAST")
    private fun reply(
        message: JsonObject,
        actions: MutableList<() -> Unit>,
        completions: MutableList<() -> Unit>,
        groups: MutableMap<Long, PushGroup<Any?>>,
    ) {
        val id = message["id"]?.jsonPrimitive?.longOrNull
        if (id == null) {
            // A session event, not a reply: `{"profile": "gone", id}`. The profiles view's next
            // snapshot carries what changed; the profile's keystore entries go here.
            if (message["profile"] != null) {
                CoreLog.d("session event: profile ${message["profile"]?.jsonPrimitive?.contentOrNull}")
                val gone = message["id"]?.jsonPrimitive?.contentOrNull
                if (message["profile"]?.jsonPrimitive?.contentOrNull == "gone" && gone != null) {
                    actions += { onProfileGone?.invoke(gone) }
                }
            } else {
                CoreLog.w("reply without an id: ${message["error"]?.jsonPrimitive?.contentOrNull}")
            }
            return
        }
        val ok = message["ok"]?.jsonPrimitive?.booleanOrNull == true
        val result = message["result"] as? JsonObject
        val waiting = pending[id]
        if (waiting == null) {
            // Nobody waits any more (timed out or cancelled). A subscription that opened anyway
            // must be closed, or the core keeps its view and its work for the whole process.
            val orphan = result?.get("sub")?.jsonPrimitive?.longOrNull
            if (ok && orphan != null) {
                early.remove(orphan)
                val born = epoch
                completions.add { unsubscribe(orphan, born) }
            }
            return
        }
        val error = message["error"]?.jsonPrimitive?.contentOrNull ?: "request failed"
        val code = message["code"]?.jsonPrimitive?.contentOrNull
        when (waiting) {
            is Pending.Plain -> {
                completions.add {
                    if (ok) {
                        waiting.deferred.complete(message["result"] ?: JsonObject(emptyMap()))
                    } else {
                        waiting.deferred.completeExceptionally(
                            CoreError(
                                error,
                                code = code,
                                retryAfterMs = message["retryAfterMs"]?.jsonPrimitive?.longOrNull,
                                views =
                                    (message["views"] as? JsonArray)
                                        ?.mapNotNull { it.jsonPrimitive.contentOrNull }
                                        .orEmpty(),
                            ),
                        )
                    }
                }
            }

            is Pending.Subscribe -> {
                if (!ok) {
                    completions.add { waiting.deferred.completeExceptionally(CoreError(error)) }
                    return
                }
                val sub = result?.get("sub")?.jsonPrimitive?.longOrNull
                if (result == null || sub == null) {
                    completions.add {
                        waiting.deferred.completeExceptionally(CoreError("the core answered subscribe without an id"))
                    }
                    return
                }
                val binding = waiting.binding as ViewBinding<Any?, Any?>
                subs[sub] = binding
                actions.add {
                    try {
                        binding.sink.onSnapshot(
                            json.decodeFromJsonElement(binding.snapshot, result.getValue("snapshot")),
                        )
                    } catch (e: Exception) {
                        CoreLog.e("cannot decode the snapshot of sub $sub: ${CoreLog.describe(e)}")
                        binding.sink.onFailure("protocol mismatch: the snapshot could not be decoded")
                    }
                }
                early.remove(sub)?.forEach { held -> push(sub, held, actions, groups) }
                completions.add { waiting.deferred.complete(sub) }
            }
        }
    }

    private fun <S, U> guarded(
        binding: ViewBinding<S, U>,
        block: () -> Unit,
    ) {
        try {
            block()
        } catch (e: Exception) {
            CoreLog.e("cannot apply an update: ${CoreLog.describe(e)}")
            binding.sink.onFailure("desync: an update did not fit the list")
        }
    }

    override fun close() {
        scope.cancel()
        inbox.close()
    }

    private companion object {
        const val EARLY_LIMIT = 64
    }
}

/** A live view's handle: commands go to the view, [close] ends it and cancels its work in the core. */
class Subscription internal constructor(
    private val client: CoreClient,
    val sub: Long,
    private val born: Int,
) {
    /** False once the core this subscription belongs to has died. */
    val alive: Boolean get() = born == client.epoch

    suspend fun command(command: JsonObject): JsonElement {
        if (!alive) throw CoreError("The core stopped.")
        return client.request("command", mapOf("sub" to JsonPrimitive(sub), "command" to command))
    }

    fun post(command: JsonObject) {
        if (!alive) return CoreLog.w("a command for a view of a stopped core was dropped")
        client.post("command", mapOf("sub" to JsonPrimitive(sub), "command" to command))
    }

    fun close() = client.unsubscribe(sub, born)
}

/**
 * The core writes pushes as `{"sub":<id>,"updates":[...]}` (serde_json sorts keys). Reading the
 * id off the front lets the client decode the updates without building a JSON tree first; any
 * other shape falls back to the tree.
 */
internal object PushHead {
    class Head(
        val sub: Long,
        val updatesStart: Int,
    )

    private const val PREFIX = "{\"sub\":"
    private const val MIDDLE = ",\"updates\":"

    fun scan(text: String): Head? {
        if (!text.startsWith(PREFIX) || !text.endsWith("]}")) return null
        val digits = PREFIX.length
        var i = digits
        var sub = 0L
        while (i < text.length && text[i] in '0'..'9') {
            sub = sub * 10 + (text[i] - '0')
            i++
        }
        if (i == digits || !text.startsWith(MIDDLE, i)) return null
        return Head(sub, i + MIDDLE.length)
    }
}
