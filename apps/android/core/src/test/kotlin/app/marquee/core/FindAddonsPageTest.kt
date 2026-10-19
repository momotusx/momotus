package app.marquee.core

import app.marquee.core.FindAddons.Action
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Configuration
import app.marquee.core.model.Installed
import app.marquee.core.model.ListedAddon
import app.marquee.core.session.CoreOps
import app.marquee.core.views.AddonCatalogView
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The page's wiring against a client: what a press sends, and what Refresh and Try again open. */
class FindAddonsPageTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val sent = CopyOnWriteArrayList<JsonObject>()
    private val subs = AtomicLong(1)

    @BeforeTest
    fun attach() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                when (msg.op) {
                    "subscribe" -> """{"sub":${subs.getAndIncrement()},"snapshot":{"lists":[]}}"""
                    "installListed" -> """{"id":"org.example","name":"Example"}"""
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

    private val JsonObject.op get() = this["op"]?.jsonPrimitive?.content

    private suspend fun until(
        what: String,
        check: () -> Boolean,
    ) {
        withTimeout(5_000) { while (!check()) delay(5) }
        assertTrue(check(), what)
    }

    private fun entry(
        installed: Installed = Installed.Absent,
        configuration: Configuration = Configuration.None,
        copyKey: String? = null,
    ) = ListedAddon(
        key = "https://a.example/manifest.json",
        id = "org.example",
        host = "a.example",
        name = "Example",
        configuration = configuration,
        configureUrl = "https://a.example/configure",
        installed = installed,
        copyKey = copyKey,
    )

    private fun page(cards: InstallCards = InstallCards(scope)) =
        FindAddonsPage(CoreOps(client), cards) { fresh -> AddonCatalogView(client, scope, fresh).also { it.open() } }

    @Test
    fun `a press installs the listed key, or turns on the link its action names`() =
        runBlocking {
            val page = page()
            val absent = entry()
            page.press(absent, FindAddons.cardAction(absent, null))!!.join()
            val install = sent.last { it.op == "installListed" }
            assertEquals("https://a.example/manifest.json", install.getValue("key").jsonPrimitive.content)

            val copy =
                entry(Installed.CopyOff, Configuration.Required, copyKey = "https://a.example/set/manifest.json")
            page.press(copy, FindAddons.cardAction(copy, null))!!.join()
            val turnOn = sent.last { it.op == "setAddonEnabled" }
            assertEquals("https://a.example/set/manifest.json", turnOn.getValue("url").jsonPrimitive.content)
            assertEquals(JsonPrimitive(true), turnOn.getValue("enabled"))

            assertNull(page.press(entry(Installed.On), Action.None("Installed")))
            assertNull(page.press(absent, Action.Configure))
            page.close()
        }

    @Test
    fun `Try again opens the view again with fresh, closes the old one and keeps running cards`() =
        runBlocking {
            val cards = InstallCards(scope)
            val page = page(cards)
            page.view.opened.await()
            val running = CompletableDeferred<Unit>()
            cards.run("https://b.example/manifest.json") { running.await() }
            cards.run("https://c.example/manifest.json") {
                throw CoreError("This addon's server could not be reached", code = "unreachable")
            }
            until("the failure is kept") { cards["https://c.example/manifest.json"]?.code == "unreachable" }

            page.retry()
            page.view.opened.await()
            val subscribes = sent.filter { it.op == "subscribe" }
            assertEquals(
                listOf("addonCatalog", "addonCatalog"),
                subscribes.map { it.getValue("view").jsonPrimitive.content },
            )
            assertEquals(JsonObject(emptyMap()), subscribes[0].getValue("params"))
            assertEquals(JsonObject(mapOf("fresh" to JsonPrimitive(true))), subscribes[1].getValue("params"))
            until("the first subscription closed") { sent.any { it.op == "unsubscribe" } }
            assertEquals(
                1L,
                sent
                    .first { it.op == "unsubscribe" }
                    .getValue("sub")
                    .jsonPrimitive.long,
            )
            assertNull(cards["https://c.example/manifest.json"])
            assertEquals(true, cards["https://b.example/manifest.json"]?.busy)
            running.complete(Unit)
            page.close()
        }
}
