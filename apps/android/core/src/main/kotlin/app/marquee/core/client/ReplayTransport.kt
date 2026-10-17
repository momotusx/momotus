package app.marquee.core.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/** One line of a recorded trace: `{"t": ms, "dir": "out" | "in", "msg": {...}}`. */
class TraceLine(
    val dir: String,
    val msg: JsonObject,
)

object Trace {
    fun parse(
        text: String,
        json: Json = CoreJson,
    ): List<TraceLine> =
        text
            .lineSequence()
            .filter { it.isNotBlank() }
            .map { line ->
                val obj = json.parseToJsonElement(line).jsonObject
                TraceLine(obj.getValue("dir").jsonPrimitive.content, obj.getValue("msg").jsonObject)
            }.toList()
}

/**
 * Plays a recorded trace back as if it were the core. A request is answered with the messages
 * recorded after the first not yet used request of the same op (and view, for `subscribe`),
 * with request ids and subscription ids rewritten to the live ones; `command` and
 * `unsubscribe` are simply acknowledged. Used by the contract tests and the bridge benchmark.
 */
class ReplayTransport(
    trace: List<TraceLine>,
    private val subOffset: Long = 0,
    private val deliver: (String) -> Unit,
) : CoreTransport {
    private class Turn(
        val key: String,
        val replies: List<JsonObject>,
    )

    private val turns: MutableList<Turn> =
        buildList {
            var key: String? = null
            var replies = mutableListOf<JsonObject>()

            fun close() {
                key?.let { add(Turn(it, replies)) }
            }
            for (line in trace) {
                if (line.dir == "out") {
                    close()
                    key = keyOf(line.msg)
                    replies = mutableListOf()
                } else {
                    replies.add(line.msg)
                }
            }
            close()
        }.toMutableList()

    private fun keyOf(request: JsonObject): String {
        val op = request["op"]?.jsonPrimitive?.content ?: ""
        return if (op == "subscribe") "subscribe:${request["view"]?.jsonPrimitive?.content}" else op
    }

    override fun send(text: String) {
        val request = CoreJson.parseToJsonElement(text).jsonObject
        val id = request["id"]?.jsonPrimitive?.longOrNull ?: return
        val key = keyOf(request)
        if (key == "command" || key == "unsubscribe") {
            deliver("""{"id":$id,"ok":true,"result":{}}""")
            return
        }
        val index = turns.indexOfFirst { it.key == key }
        if (index < 0) return
        for (message in turns.removeAt(index).replies) deliver(rewrite(message, id).toString())
    }

    private fun rewrite(
        message: JsonObject,
        id: Long,
    ): JsonObject {
        val out = HashMap<String, JsonElement>(message)
        if (message.containsKey("ok")) out["id"] = JsonPrimitive(id)
        message["sub"]?.jsonPrimitive?.longOrNull?.let { out["sub"] = JsonPrimitive(it + subOffset) }
        val result = message["result"] as? JsonObject
        val sub = result?.get("sub")?.jsonPrimitive?.longOrNull
        if (result != null && sub != null) {
            val shifted = HashMap<String, JsonElement>(result)
            shifted["sub"] = JsonPrimitive(sub + subOffset)
            out["result"] = JsonObject(shifted)
        }
        return JsonObject(out)
    }
}

/**
 * Plays the trace again for every `subscribe` the client sends, each time with fresh subscription
 * ids, so one client can open the same view many times (the bridge benchmark and the tests).
 */
class RepeatingReplay(
    private val trace: List<TraceLine>,
    private val deliver: (String) -> Unit,
) : CoreTransport {
    private var runs = 0L
    private var current = ReplayTransport(trace, 0, deliver)

    @Synchronized
    override fun send(text: String) {
        if (CoreJson
                .parseToJsonElement(text)
                .jsonObject["op"]
                ?.jsonPrimitive
                ?.content == "subscribe"
        ) {
            runs += 1
            current = ReplayTransport(trace, 1000L * runs, deliver)
        }
        current.send(text)
    }
}
