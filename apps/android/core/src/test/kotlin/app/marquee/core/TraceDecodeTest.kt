package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.client.Trace
import app.marquee.core.model.AddonsSnapshot
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.HomeUpdate
import app.marquee.core.model.LibrarySnapshot
import app.marquee.core.model.PlaybackSnapshot
import app.marquee.core.model.Prefs
import app.marquee.core.model.SearchSnapshot
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleUpdate
import app.marquee.core.model.VersionsSnapshot
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlin.test.Test
import kotlin.test.assertTrue

/**
 * Golden traces recorded from a real core with real addons (scripts/capture-trace.ts). Every
 * snapshot and every update is decoded strictly (unknown keys fail), so a protocol change shows
 * up here before it shows up on a screen.
 */
class TraceDecodeTest {
    private val strict = Json(CoreJson) { ignoreUnknownKeys = false }

    private class Serializers(
        val snapshot: KSerializer<*>,
        val update: KSerializer<*>,
    )

    private val views =
        mapOf(
            "home" to Serializers(HomeSnapshot.serializer(), HomeUpdate.serializer()),
            "title" to Serializers(TitleSnapshot.serializer(), TitleUpdate.serializer()),
            "versions" to Serializers(VersionsSnapshot.serializer(), VersionsSnapshot.serializer()),
            "playback" to Serializers(PlaybackSnapshot.serializer(), PlaybackSnapshot.serializer()),
            "search" to Serializers(SearchSnapshot.serializer(), SearchSnapshot.serializer()),
            "discover" to Serializers(DiscoverSnapshot.serializer(), DiscoverSnapshot.serializer()),
            "library" to Serializers(LibrarySnapshot.serializer(), LibrarySnapshot.serializer()),
            "addons" to Serializers(AddonsSnapshot.serializer(), AddonsSnapshot.serializer()),
            "settings" to Serializers(Prefs.serializer(), Prefs.serializer()),
        )

    private fun load(name: String) = Trace.parse(javaClass.getResource("/traces/$name.jsonl")!!.readText(), strict)

    private fun decodeAll(name: String): Int {
        var decoded = 0
        val viewOfRequest = HashMap<Long, String>()
        val viewOfSub = HashMap<Long, String>()
        for (line in load(name)) {
            val msg = line.msg
            if (line.dir == "out") {
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    viewOfRequest[msg.getValue("id").jsonPrimitive.long] = msg.getValue("view").jsonPrimitive.content
                }
                continue
            }
            val result = msg["result"] as? kotlinx.serialization.json.JsonObject
            val snapshot = result?.get("snapshot")
            if (snapshot != null) {
                val view = viewOfRequest.getValue(msg.getValue("id").jsonPrimitive.long)
                viewOfSub[result.getValue("sub").jsonPrimitive.long] = view
                strict.decodeFromJsonElement(views.getValue(view).snapshot, snapshot)
                decoded++
            }
            val updates = msg["updates"] as? JsonArray
            if (updates != null) {
                val view = viewOfSub.getValue(msg.getValue("sub").jsonPrimitive.long)
                strict.decodeFromJsonElement(ListSerializer(views.getValue(view).update), updates)
                decoded += updates.size
            }
        }
        return decoded
    }

    @Test fun `home`() = assertTrue(decodeAll("home") >= 3)

    @Test fun `title of a movie`() = assertTrue(decodeAll("title-movie") >= 2)

    @Test fun `title of a series with episodes`() = assertTrue(decodeAll("title-series") >= 3)

    @Test fun `discover`() = assertTrue(decodeAll("discover") >= 3)

    @Test fun `search`() = assertTrue(decodeAll("search") >= 2)

    @Test fun `library`() = assertTrue(decodeAll("library") >= 2)

    @Test fun `addons`() = assertTrue(decodeAll("addons") >= 1)

    @Test fun `versions and playback`() = assertTrue(decodeAll("versions-playback") >= 5)

    @Test
    fun `settings and the setPrefs reply`() {
        assertTrue(decodeAll("settings") >= 1)
        val trace = load("settings")
        val request = trace.first { it.dir == "out" && it.msg["op"]?.jsonPrimitive?.content == "setPrefs" }
        val id =
            request.msg
                .getValue("id")
                .jsonPrimitive.long
        val reply = trace.first { it.dir == "in" && it.msg["id"]?.jsonPrimitive?.long == id }
        strict.decodeFromJsonElement(Prefs.serializer(), reply.msg.getValue("result"))
    }

    @Test
    fun `home updates decode to row diffs`() {
        val updates =
            load("home").flatMap { line ->
                (line.msg["updates"] as? JsonArray).orEmpty().map {
                    strict.decodeFromJsonElement(
                        HomeUpdate.serializer(),
                        it,
                    )
                }
            }
        assertTrue(updates.any { it is HomeUpdate.Rows })
    }
}
