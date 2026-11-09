package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The profile step's and the setup sheet's writes (plans/first-screens.md 3.4, 3.5): what each press sends. */
class FirstSetupTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val ops = CoreOps(client)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    /** Ops that fail; and a held answer for one op, until completed. */
    private val failing = mutableSetOf<String>()

    @Volatile
    private var hold: Pair<String, CompletableDeferred<Unit>>? = null

    private val defaults =
        """{"version":1,"addons":[""" +
            """{"key":"cinemeta","name":"Cinemeta","line":"Titles.","group":"catalogs","installed":"on"},""" +
            """{"key":"kitsu","name":"Kitsu","line":"Anime.","group":"catalogs","installed":"off"},""" +
            """{"key":"torrentio","name":"Torrentio","line":"Streams.","group":"streams","installed":"absent"}]}"""

    private val profiles = """{"profiles":[],"active":"default","locked":false,"max":5,"kids":false}"""

    /** A profile's prefs as the core answers `setPrefs` (core/src/test/resources/traces/settings.jsonl). */
    private val prefs =
        CoreJson
            .parseToJsonElement(File("src/test/resources/traces/settings.jsonl").readLines()[1])
            .jsonObject
            .getValue("msg")
            .jsonObject
            .getValue("result")
            .jsonObject
            .getValue("snapshot")
            .toString()

    @BeforeTest
    fun attach() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val op = msg.getValue("op").jsonPrimitive.content

            fun answer() {
                if (op in failing) {
                    client.onMessage("""{"id":$id,"ok":false,"error":"No","code":"internal"}""")
                    return
                }
                val result =
                    when (op) {
                        "defaultAddons" -> defaults
                        "finishProfileStep" -> profiles
                        "setPrefs" -> prefs
                        else -> "{}"
                    }
                client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
            }
            val held = hold
            if (held != null && held.first == op) {
                scope.launch {
                    held.second.await()
                    answer()
                }
            } else {
                answer()
            }
        }
    }

    @AfterTest
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private fun sent(op: String) = sent.filter { it.getValue("op").jsonPrimitive.content == op }

    @Test
    fun `Start watching sends the shown chip and no streams for Start fast, then every default not turned off`() =
        runBlocking {
            val setup = FirstSetup(ops, scope)
            var import: Boolean? = null
            setup.start("en") { import = it }!!.join()
            val patch = sent("setPrefs").single().getValue("patch").jsonObject
            assertEquals(setOf("subtitleLanguages"), patch.keys)
            assertEquals(listOf("en"), patch.getValue("subtitleLanguages").jsonArray.map { it.jsonPrimitive.content })
            val choices =
                sent("installDefaults")
                    .single()
                    .getValue("choices")
                    .jsonArray
                    .map { it.jsonObject }
            assertEquals(
                listOf("cinemeta" to "true", "kitsu" to "false", "torrentio" to "true"),
                choices.map { it.getValue("key").jsonPrimitive.content to it.getValue("on").jsonPrimitive.content },
            )
            assertEquals(false, import)
        }

    @Test
    fun `Best quality adds its streams, No subtitles writes none, and the Stremio library waits for Import`() =
        runBlocking {
            val setup = FirstSetup(ops, scope)
            setup.best = true
            setup.bring = true
            var import: Boolean? = null
            setup.start(FirstRun.NONE) { import = it }!!.join()
            val patch = sent("setPrefs").single().getValue("patch").jsonObject
            assertEquals(JsonArray(emptyList()), patch.getValue("subtitleLanguages"))
            val streams = patch.getValue("streams").jsonObject
            assertEquals(JsonNull, streams.getValue("sizeLimitBytes"))
            assertEquals("20", streams.getValue("minSeeders").jsonPrimitive.content)
            assertTrue(sent("installDefaults").isEmpty())
            assertEquals(true, import)
            // Import's Done: the defaults the import did not bring.
            var home = false
            setup.afterImport { home = true }!!.join()
            assertEquals(1, sent("installDefaults").size)
            assertTrue(home)
        }

    @Test
    fun `a second press while the first is on its way sends nothing`() =
        runBlocking {
            val setup = FirstSetup(ops, scope)
            val gate = CompletableDeferred<Unit>()
            hold = "setPrefs" to gate
            val first = setup.start("tr") {}
            assertNull(setup.start("tr") {})
            assertNull(setup.skip {})
            gate.complete(Unit)
            first!!.join()
            assertEquals(1, sent("setPrefs").size)
            assertEquals(1, sent("installDefaults").size)
        }

    @Test
    fun `a failed part is named and every answer is kept for Try again`() =
        runBlocking {
            val setup = FirstSetup(ops, scope)
            setup.best = true
            setup.picked = "de"
            failing += "installDefaults"
            var done = false
            setup.start("de") { done = true }!!.join()
            assertEquals(FirstSetup.Part.Sources, setup.failed)
            assertFalse(done)
            assertTrue(setup.best)
            assertEquals("de", setup.picked)
            failing.clear()
            failing += "setPrefs"
            setup.start("de") { done = true }!!.join()
            assertEquals(FirstSetup.Part.Prefs, setup.failed)
            failing.clear()
            setup.start("de") { done = true }!!.join()
            assertNull(setup.failed)
            assertTrue(done)
        }

    @Test
    fun `Skip installs the defaults and writes no prefs`() =
        runBlocking {
            val setup = FirstSetup(ops, scope)
            var done = false
            setup.skip { done = true }!!.join()
            assertTrue(sent("setPrefs").isEmpty())
            assertEquals(1, sent("installDefaults").size)
            assertTrue(done)
        }

    @Test
    fun `an untouched Continue sends an empty name and avatar, once`() =
        runBlocking {
            val step = FirstProfileStep(ops, scope)
            val gate = CompletableDeferred<Unit>()
            hold = "finishProfileStep" to gate
            var done = 0
            val first = step.finish("  ", "") { done++ }
            assertNull(step.finish("", "") { done++ })
            gate.complete(Unit)
            first!!.join()
            val asked = sent("finishProfileStep").single()
            assertEquals("", asked.getValue("name").jsonPrimitive.content)
            assertEquals("", asked.getValue("avatar").jsonPrimitive.content)
            assertEquals(1, done)
        }
}
