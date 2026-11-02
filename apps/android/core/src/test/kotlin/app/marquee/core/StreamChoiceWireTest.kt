package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.client.TitleFacts
import app.marquee.core.client.versionsParams
import app.marquee.core.model.Episode
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.Network
import app.marquee.core.model.TitleDetails
import app.marquee.core.session.CoreOps
import app.marquee.core.session.LifecycleEvent
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/** What the app sends the core for stream choice (plans/m13-stream-choice.md 4.3): the params and the ops. */
class StreamChoiceWireTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    private val conditions =
        """{"network":"wifi","http":{"mbps":42.2,"samples":3},"p2p":{"mbps":10,"samples":0},"freeBytes":null,"engine":false}"""
    private val zero = """{"ready":0,"height":0,"hdr":0,"size":0,"picks":0,"epoch":0,"atMs":0}"""
    private val prefs =
        """{"streams":{"quality":"balanced","minSeeders":50,"hdr":true,"preferReady":true,"learn":true},""" +
            """"autoplayNext":true,"homeRows":{},"learned":{"uhd":$zero,"fhd":$zero,"hd":$zero,"sd":$zero},"learnedEpoch":2}"""

    /** A core that records each request and answers it as the bridge would. */
    private fun answering() {
        client.attach(
            CoreTransport { text ->
                val request = CoreJson.parseToJsonElement(text).jsonObject
                sent += request
                val result =
                    when (request.getValue("op").jsonPrimitive.content) {
                        "streamConditions", "resetSpeed" -> conditions
                        "undoLearned", "forgetLearned" -> prefs
                        else -> "{}"
                    }
                client.onMessage("""{"id":${request.getValue("id").jsonPrimitive.long},"ok":true,"result":$result}""")
            },
        )
    }

    private suspend fun until(check: () -> Boolean) = withTimeout(5_000) { while (!check()) delay(5) }

    private fun JsonObject.text(key: String) = getValue(key).jsonPrimitive.content

    @Test
    fun `the ops go out with the names and fields the bridge reads, and their answers decode`() =
        runBlocking {
            answering()
            val ops = CoreOps(client)
            assertEquals(Network.Wifi, ops.streamConditions().network)
            assertEquals(42.2f, ops.resetSpeed().http.mbps)
            assertEquals(2, ops.undoLearned(1080, LearnedChoice.Size).learnedEpoch)
            assertEquals(2, ops.forgetLearned().learnedEpoch)
            assertEquals(
                listOf("streamConditions", "resetSpeed", "undoLearned", "forgetLearned"),
                sent.map { it.text("op") },
            )
            assertEquals("1080", sent[2].text("screenClass"))
            assertEquals("size", sent[2].text("choice"))
        }

    @Test
    fun `a network change says the new network's type`() =
        runBlocking {
            answering()
            val ops = CoreOps(client)
            ops.lifecycle(LifecycleEvent.NetworkChanged, Network.Cellular)
            ops.lifecycle(LifecycleEvent.Foreground)
            until { sent.size == 2 }
            assertEquals("networkChanged", sent[0].text("event"))
            assertEquals("cellular", sent[0].text("network"))
            assertFalse("network" in sent[1], "an event without a network names none")
        }

    private val caps = DeviceCapsParams(displayHeight = 2160, hdr = listOf("hdr10"), network = Network.Ethernet)

    @Test
    fun `the versions params carry the network and what the app knows of the title`() {
        val facts = TitleFacts("52 min", "2008-2013", "2013-09-29", 16)
        val params = versionsParams("series", "tt1", "tt1:5:16", "Breaking Bad", caps, 5, 16, facts = facts)
        assertEquals("ethernet", params.text("network"))
        // The title's id goes with every open, not only a download's: a kids profile is refused without it.
        assertEquals("tt1", params.text("itemId"))
        assertFalse("purpose" in params)
        assertEquals("52 min", params.text("runtime"))
        assertEquals("2008-2013", params.text("releaseInfo"))
        assertEquals("2013-09-29", params.text("released"))
        assertEquals(JsonPrimitive(16), params["episodesInSeason"])
        assertEquals(JsonPrimitive(2160), params["displayHeight"])
        // Nothing known: the core reads the meta, so nothing is sent rather than an empty value.
        val bare = versionsParams("movie", "tt2", "tt2", "Sintel", caps.copy(network = null))
        for (key in listOf("network", "runtime", "releaseInfo", "released", "episodesInSeason")) {
            assertFalse(key in bare, key)
        }
    }

    private fun episode(
        id: String,
        season: Int,
        released: String?,
    ) = Episode(videoId = id, season = season, episode = 1, title = id, released = released, watched = false)

    @Test
    fun `a title page knows the runtime of Play's video and each listed episode's date and season size`() {
        val details =
            TitleDetails(
                id = "tt1",
                kind = "series",
                name = "Show",
                releaseInfo = "2008-2013",
                runtime = "47 min",
                source = "cinemeta",
            )
        val episodes =
            listOf(episode("tt1:1:1", 1, "2008-01-20"), episode("tt1:1:2", 1, null), episode("tt1:0:1", 0, null))
        val facts = TitleFacts.of(details, "tt1:1:1", episodes)
        assertEquals(TitleFacts("47 min", "2008-2013", "2008-01-20", 2), facts["tt1:1:1"])
        assertEquals(TitleFacts("47 min", "2008-2013", null, 2), facts["tt1:1:2"])
        assertEquals(1, facts["tt1:0:1"]?.episodesInSeason)
        // A movie: Play's video has the title's facts and no episode's.
        assertEquals(
            mapOf("tt9" to TitleFacts("120 min", "2010")),
            TitleFacts.of(details.copy(runtime = "120 min", releaseInfo = "2010"), "tt9", emptyList()),
        )
        assertEquals(emptyMap(), TitleFacts.of(null, "tt9", episodes))
    }
}
