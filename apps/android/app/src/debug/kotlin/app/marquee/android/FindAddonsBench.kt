package app.marquee.android

import android.os.SystemClock
import android.util.Log
import app.marquee.core.FindAddons
import app.marquee.core.client.CoreJson
import app.marquee.core.model.AddonCatalogSnapshot
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * Debug builds only (`screen=bench-find-addons`): what the largest Find addons snapshot costs this device
 * (plans/plugins-and-marketplace.md 7.1, native review F3). It decodes a 3.5 MiB snapshot of large entries 20
 * times, then runs a search keystroke over a 2,000-entry list 50 times, and logs p50 and p90 in ms under the
 * tag MqBench, one line each.
 */
object FindAddonsBench {
    private fun entry(
        list: Int,
        i: Int,
    ) = buildJsonObject {
        val host = "l${list}x$i.example"
        put("key", "https://$host/manifest.json")
        put("id", "id.$list.$i.".padEnd(120, 'x'))
        put("host", host)
        put("name", "Ä Name $i ".padEnd(80, 'n'))
        put("description", "Türkçe altyazı and more words $i ".padEnd(300, 'd'))
        putJsonArray("types") { repeat(4) { add(kotlinx.serialization.json.JsonPrimitive("movie")) } }
        putJsonArray("provides") { repeat(4) { add(kotlinx.serialization.json.JsonPrimitive("subtitles")) } }
        put("configuration", "none")
        put("configureUrl", "https://$host/configure")
        put("installed", "absent")
    }

    /** Entries per list: Cinemeta's two, then three large ones; about 3.5 MiB as JSON in all. */
    private val SIZES = listOf(100, 100, 2_000, 1_300, 1_000)

    /** The snapshot as JSON. */
    private fun snapshot(): String {
        val lists =
            buildJsonArray {
                for (list in 0 until 5) {
                    add(
                        buildJsonObject {
                            put(
                                "id",
                                if (list == 0) {
                                    "official"
                                } else if (list == 1) {
                                    "community"
                                } else {
                                    "addon-$list:dir"
                                },
                            )
                            put("name", "List $list")
                            put("state", buildJsonObject { put("state", "ready") })
                            put("more", false)
                            putJsonArray("entries") { repeat(SIZES[list]) { add(entry(list, it)) } }
                        },
                    )
                }
            }
        return buildJsonObject { put("lists", lists) }.toString()
    }

    private fun percentiles(ms: List<Double>): String {
        val sorted = ms.sorted()
        val at = { p: Double -> sorted[((sorted.size - 1) * p).toInt()] }
        return "p50=%.1f p90=%.1f n=%d".format(at(0.5), at(0.9), sorted.size)
    }

    fun run(): List<String> {
        val json = snapshot()
        val decode =
            (0 until 20).map {
                val t = SystemClock.elapsedRealtimeNanos()
                CoreJson.decodeFromString(AddonCatalogSnapshot.serializer(), json)
                (SystemClock.elapsedRealtimeNanos() - t) / 1e6
            }
        val entries =
            CoreJson
                .decodeFromString(AddonCatalogSnapshot.serializer(), json)
                .lists
                .first { it.entries.size == 2_000 }
                .entries
        val foldStart = SystemClock.elapsedRealtimeNanos()
        val texts = entries.map(FindAddons::searchText)
        val fold = (SystemClock.elapsedRealtimeNanos() - foldStart) / 1e6
        val queries = listOf("t", "tu", "tur", "turk", "turkc", "turkce", "turkce a", "altyazi", "name 1", "zzz")
        val keystroke =
            (0 until 50).map { n ->
                val t = SystemClock.elapsedRealtimeNanos()
                val words = FindAddons.queryWords(queries[n % queries.size])
                entries.indices.count { FindAddons.matches(texts[it], words) }
                (SystemClock.elapsedRealtimeNanos() - t) / 1e6
            }
        val lines =
            listOf(
                "snapshot_bytes=${json.length} decode_ms ${percentiles(decode)}",
                "fold_once_ms=%.1f entries=%d".format(fold, entries.size),
                "keystroke_ms ${percentiles(keystroke)} entries=${entries.size}",
            )
        lines.forEach { Log.i("MqBench", it) }
        return lines
    }
}
