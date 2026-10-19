package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.model.LearnedChoice
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Settings > Stream choice: what each row writes, and the device line's Measure again. */
class StreamChoiceSettingsTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @AfterTest
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private fun conditions(samples: Int) =
        """{"network":"wifi","http":{"mbps":42.2,"samples":$samples},"p2p":{"mbps":10,"samples":0},"engine":false}"""

    private val zero = """{"ready":0,"height":0,"hdr":0,"size":0,"picks":0,"epoch":0,"atMs":0}"""
    private val prefs =
        """{"streams":{"quality":"balanced","minSeeders":50,"hdr":true,"preferReady":true,"learn":true},""" +
            """"autoplayNext":true,"homeRows":{},"learned":{"uhd":$zero,"fhd":$zero,"hd":$zero,"sd":$zero},"learnedEpoch":0}"""

    /** A core that answers as the bridge would; `setPrefs` fails when [refuse] is set. */
    private fun page(
        samples: Int,
        refuse: Boolean = false,
    ): StreamChoiceSettings {
        client.attach(
            CoreTransport { text ->
                val request = CoreJson.parseToJsonElement(text).jsonObject
                sent += request
                val id = request.getValue("id").jsonPrimitive.long
                val reply =
                    when (request.getValue("op").jsonPrimitive.content) {
                        "streamConditions" -> {
                            """{"id":$id,"ok":true,"result":${conditions(samples)}}"""
                        }

                        "resetSpeed" -> {
                            """{"id":$id,"ok":true,"result":${conditions(0)}}"""
                        }

                        "setPrefs" -> {
                            if (refuse) {
                                """{"id":$id,"ok":false,"error":"the profile is locked"}"""
                            } else {
                                """{"id":$id,"ok":true,"result":$prefs}"""
                            }
                        }

                        else -> {
                            """{"id":$id,"ok":true,"result":$prefs}"""
                        }
                    }
                client.onMessage(reply)
            },
        )
        return StreamChoiceSettings(CoreOps(client), scope).also { it.load() }
    }

    private suspend fun until(check: () -> Boolean) = withTimeout(5_000) { while (!check()) delay(5) }

    private fun JsonObject.op() = getValue("op").jsonPrimitive.content

    @Test
    fun `each row writes its one field under streams, a null as no limit`() =
        runBlocking {
            val page = page(samples = 0)
            page.set("sizeLimitBytes", null)
            page.set("minSeeders", 100)
            page.set("languages", listOf("tr", "en"))
            until { sent.count { it.op() == "setPrefs" } == 3 }
            val patches = sent.filter { it.op() == "setPrefs" }.map { it.getValue("patch").toString() }
            assertEquals(
                listOf(
                    """{"streams":{"sizeLimitBytes":null}}""",
                    """{"streams":{"minSeeders":100}}""",
                    """{"streams":{"languages":["tr","en"]}}""",
                ),
                patches,
            )
        }

    @Test
    fun `Measure again shows once a speed was measured, and stays after a press`() =
        runBlocking {
            val page = page(samples = 3)
            until { page.conditions != null }
            assertTrue(page.measureShown)
            page.measureAgain()
            until { page.measuring }
            assertEquals(0, page.conditions?.http?.samples)
            assertTrue(page.measureShown, "the pressed button stays, so focus does too")
            page.measureAgain()
            delay(50)
            assertEquals(1, sent.count { it.op() == "resetSpeed" }, "a second press sends nothing")
        }

    @Test
    fun `nothing measured here, nothing to measure again`() =
        runBlocking {
            val page = page(samples = 0)
            until { page.conditions != null }
            assertFalse(page.measureShown)
        }

    @Test
    fun `Undo and Forget send what Settings lists, and a refused write says why`() =
        runBlocking {
            val page = page(samples = 0, refuse = true)
            page.undo(StreamChoiceCopy.LearnedRow(2160, LearnedChoice.Height, false, "On 4K screens..."))
            page.forget()
            until { sent.any { it.op() == "forgetLearned" } }
            val undo = sent.first { it.op() == "undoLearned" }
            assertEquals("2160", undo.getValue("screenClass").jsonPrimitive.content)
            assertEquals("height", undo.getValue("choice").jsonPrimitive.content)
            page.set("hdr", false)
            until { page.problem != null }
            assertEquals("the profile is locked", page.problem)
        }

    @Test
    fun `the first run's answer writes its two fields in one patch, then goes on, once`() =
        runBlocking {
            val page = page(samples = 0)
            var went = 0
            page.answer(StreamChoiceCopy.BEST) { went++ }
            page.answer(StreamChoiceCopy.FAST) { went++ }
            until { went == 1 && !page.answering }
            assertEquals(
                listOf("""{"streams":{"sizeLimitBytes":null,"minSeeders":20}}"""),
                sent.filter { it.op() == "setPrefs" }.map { it.getValue("patch").toString() },
            )
        }

    @Test
    fun `a refused answer says why and does not go on`() =
        runBlocking {
            val page = page(samples = 0, refuse = true)
            var went = false
            page.answer(StreamChoiceCopy.FAST) { went = true }
            until { page.problem != null && !page.answering }
            assertFalse(went)
            assertEquals("the profile is locked", page.problem)
        }

    @Test
    fun `a 4K cap or none is this screen's resolution`() {
        assertTrue(StreamChoiceSettings.resolutionOn(null, null))
        assertTrue(StreamChoiceSettings.resolutionOn(null, 2160))
        assertFalse(StreamChoiceSettings.resolutionOn(null, 1080))
        assertTrue(StreamChoiceSettings.resolutionOn(1080, 1080))
        assertFalse(StreamChoiceSettings.resolutionOn(720, 1080))
    }
}
