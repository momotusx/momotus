package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.LanguageChoice
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** The ops of the core's journey work (4e5fd336) as the app sends and reads them. */
class JourneyOpsTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val ops = CoreOps(client)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @Volatile
    private var moved = false

    init {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                when (msg["op"]?.jsonPrimitive?.content) {
                    "lifecycle" -> """{"serverMoved":$moved}"""
                    "languages" -> """[{"code":"en","name":"English"},{"code":"pt-BR","name":"Portuguese (Brazil)"}]"""
                    else -> "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
    }

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `foreground says whether the stream server moved`() =
        runBlocking {
            assertFalse(ops.foreground())
            moved = true
            assertTrue(ops.foreground())
            assertEquals("foreground", sent.last()["event"]?.jsonPrimitive?.content)
        }

    @Test
    fun `Remove from Continue Watching names the title`() =
        runBlocking {
            ops.removeFromContinue("tt0944947")
            val msg = sent.last()
            assertEquals("removeFromContinue", msg["op"]?.jsonPrimitive?.content)
            assertEquals("tt0944947", msg["itemId"]?.jsonPrimitive?.content)
        }

    @Test
    fun `the language list reads as code and name`() =
        runBlocking {
            assertEquals(
                listOf(LanguageChoice("en", "English"), LanguageChoice("pt-BR", "Portuguese (Brazil)")),
                ops.languages(),
            )
        }
}
