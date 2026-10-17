package app.marquee.android

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.TimeUnit

/**
 * P0 bridge spike. Proves, in order: the library loads, `version()` answers, Cinemeta installs
 * over real HTTPS (the platform certificate verifier works), Home rows arrive, and what a
 * message round trip costs with and without permanently attached callback threads.
 */
class Spike(private val app: MarqueeApp, private val log: (String) -> Unit) {
    private val core by lazy { app.core.get(30, TimeUnit.SECONDS) }
    private var nextId = 1L

    private fun ms(from: Long) = (System.nanoTime() - from) / 1_000_000

    /** Sends a request and waits for its reply, collecting pushes seen meanwhile. */
    private fun request(op: String, fields: Map<String, Any?> = emptyMap(), timeoutMs: Long = 20_000): JsonObject {
        val id = nextId++
        val body = buildString {
            append("{\"id\":$id,\"op\":\"$op\"")
            for ((k, v) in fields) {
                append(",\"$k\":")
                append(if (v is String) "\"$v\"" else v.toString())
            }
            append("}")
        }
        core.send(body)
        val deadline = System.nanoTime() + timeoutMs * 1_000_000
        while (System.nanoTime() < deadline) {
            val (_, raw) = app.inbox.poll(100, TimeUnit.MILLISECONDS) ?: continue
            val message = Json.parseToJsonElement(raw).jsonObject
            if (message["id"]?.jsonPrimitive?.contentOrNull == id.toString()) return message
        }
        error("no reply to $op")
    }

    fun run() {
        try {
            log("keep callback threads attached: ${app.keepAttached}")
            log("core open: ${app.core.get(30, TimeUnit.SECONDS).let { app.openMs }} ms")
            log("version: ${core.version()}")

            var t = System.nanoTime()
            val install = request("installAddon", mapOf("url" to "https://v3-cinemeta.strem.io/manifest.json"))
            log("installAddon Cinemeta: ${ms(t)} ms ok=${install["ok"]} ${install["result"] ?: install["error"]}")

            t = System.nanoTime()
            val sub = request("subscribe", mapOf("view" to "home", "params" to "{}"))
            val result = sub["result"]!!.jsonObject
            val rows = result["snapshot"]!!.jsonObject["rows"]!!.jsonArray
            log("subscribe home: ${ms(t)} ms, ${rows.size} rows in the first snapshot")

            t = System.nanoTime()
            var pushes = 0
            var ready = 0
            var firstReady = -1L
            val until = System.nanoTime() + 8_000_000_000L
            while (System.nanoTime() < until) {
                val (_, raw) = app.inbox.poll(100, TimeUnit.MILLISECONDS) ?: continue
                val message = Json.parseToJsonElement(raw).jsonObject
                if (message["sub"] == null) continue
                pushes++
                for (update in message["updates"]!!.jsonArray) {
                    val diffs = update.jsonObject["diffs"]?.jsonArray ?: continue
                    for (diff in diffs) {
                        val value = diff.jsonObject["value"]?.jsonObject
                        if (value != null && (value["state"] as? JsonObject)?.get("state") == JsonPrimitive("ready")) {
                            ready++
                            if (firstReady < 0) firstReady = ms(t)
                        }
                    }
                }
            }
            log("home pushes in 8 s: $pushes, row updates with state ready: $ready, first ready after $firstReady ms")

            roundTrips()
            cleartext()
        } catch (e: Throwable) {
            log("spike failed: $e")
        }
    }

    /** V2: the Java stack refuses cleartext except to 127.0.0.1 (network security config). */
    private fun cleartext() {
        for (url in listOf("http://example.com/", "http://127.0.0.1:9/")) {
            val result = try {
                java.net.URL(url).openConnection().apply { connectTimeout = 3000 }.connect()
                "connected"
            } catch (e: Exception) {
                "${e.javaClass.simpleName}: ${e.message}"
            }
            log("cleartext $url -> $result")
        }
    }

    /** Sequential round trips of a request that never touches the network. */
    private fun roundTrips() {
        val samples = LongArray(300)
        for (i in samples.indices) {
            val id = nextId++
            val t = System.nanoTime()
            core.send("{\"id\":$id,\"op\":\"unsubscribe\",\"sub\":999999}")
            while (true) {
                val (arrived, raw) = app.inbox.poll(5, TimeUnit.SECONDS) ?: error("no reply")
                if (raw.contains("\"id\":$id,")) {
                    samples[i] = arrived - t
                    break
                }
            }
        }
        val sorted = samples.drop(50).sorted()
        val p = { q: Double -> sorted[((sorted.size - 1) * q).toInt()] / 1000 }
        log("round trip (250 samples, after warmup): p50 ${p(0.5)} us, p90 ${p(0.9)} us, p99 ${p(0.99)} us, max ${sorted.last() / 1000} us")
        log("attached=${app.keepAttached}")
    }
}
