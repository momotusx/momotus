package app.marquee.core

import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.route.DefaultsMode
import app.marquee.core.session.CoreOps
import app.marquee.core.views.AddonsView
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The recommended addons wired to a client: what the step, the Home card and the start decision send. */
class DefaultsTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val ops = CoreOps(client)
    private val sent = CopyOnWriteArrayList<JsonObject>()
    private val subs = AtomicLong(1)

    /** The next `installDefaults` gets no answer (the core died or the answer was lost). */
    @Volatile
    private var loseNextInstall = false

    /** Holds `installDefaults` answers until completed. */
    @Volatile
    private var installGate: CompletableDeferred<Unit>? = null

    @Volatile
    private var fresh = false

    private val defaults =
        """{"version":1,"addons":[""" +
            """{"key":"cinemeta","name":"Cinemeta","line":"Titles.","group":"catalogs","installed":"on"},""" +
            """{"key":"torrentio","name":"Torrentio","line":"Streams.","group":"streams","installed":"absent"},""" +
            """{"key":"trakt","name":"Trakt","line":"Lists.","group":"lists","installed":"absent"}]}"""

    @BeforeTest
    fun attach() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach

            fun reply(result: String) = client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
            when (msg.op) {
                "defaultAddons" -> {
                    reply(defaults)
                }

                "installDefaults" -> {
                    if (loseNextInstall) {
                        loseNextInstall = false
                    } else {
                        val gate = installGate
                        if (gate == null) {
                            reply("{}")
                        } else {
                            scope.launch {
                                gate.await()
                                reply("{}")
                            }
                        }
                    }
                }

                "subscribe" -> {
                    reply("""{"sub":${subs.getAndIncrement()},"snapshot":{"addons":[],"fresh":$fresh}}""")
                }

                else -> {
                    reply("{}")
                }
            }
        }
    }

    @AfterTest
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private val JsonObject.op get() = this["op"]?.jsonPrimitive?.content

    private fun sentOps(op: String) = sent.filter { it.op == op }

    @Test
    fun `a second Continue while the first is on its way sends nothing`() =
        runBlocking {
            val step = DefaultsStep(ops, scope)
            step.load().join()
            installGate = CompletableDeferred()
            var done = 0
            val first = step.save(DefaultsMode.FirstRun) { done++ }
            assertNotNull(first)
            assertTrue(step.busy)
            assertNull(step.save(DefaultsMode.FirstRun) { done++ })
            installGate!!.complete(Unit)
            first.join()
            assertEquals(1, sentOps("installDefaults").size)
            assertEquals(1, done)
            assertFalse(step.busy)
        }

    @Test
    fun `after a lost answer Continue sends the same switches again`() =
        runBlocking {
            val step = DefaultsStep(ops, scope)
            step.load().join()
            step.switches["trakt"] = false
            loseNextInstall = true
            var done = 0
            // No answer: the request times out, the step says so and keeps the switches.
            step.save(DefaultsMode.FirstRun) { done++ }!!.join()
            assertNotNull(step.failed)
            assertEquals(0, done)
            assertFalse(step.busy)
            step.save(DefaultsMode.FirstRun) { done++ }!!.join()
            assertEquals(1, done)
            assertNull(step.failed)
            val (lost, again) = sentOps("installDefaults")
            // Absolute states, all three named: if the lost one did land, the core writes nothing now.
            assertEquals(lost["choices"], again["choices"])
            assertEquals(
                CoreJson.parseToJsonElement(
                    """[{"key":"cinemeta","on":true},{"key":"torrentio","on":true},{"key":"trakt","on":false}]""",
                ),
                again["choices"],
            )
        }

    @Test
    fun `Review or Not now writes defaultsSeen once, and the card stays gone`() =
        runBlocking {
            val card = DefaultsCard(ops, scope)
            card.check(kids = false) { 0 }
            assertNotNull(card.offer)
            card.answer()!!.join()
            assertNull(card.offer)
            val writes = sentOps("setPrefs")
            assertEquals(1, writes.size)
            assertEquals(JsonObject(mapOf("defaultsSeen" to JsonPrimitive(1))), writes.single()["patch"])
            // Home asks again before the write is read back: the answered version stays answered.
            card.check(kids = false) { 0 }
            assertNull(card.offer)
            assertNull(card.answer())
            assertEquals(1, sentOps("setPrefs").size)
        }

    @Test
    fun `a profile that saw this version gets no card`() =
        runBlocking {
            val card = DefaultsCard(ops, scope)
            card.check(kids = false) { 1 }
            assertNull(card.offer)
            assertNull(card.answer())
            assertTrue(sentOps("setPrefs").isEmpty())
        }

    @Test
    fun `the start reads fresh, not an empty list`() =
        runBlocking {
            // Compose's frame loop sends snapshot changes on a device; here a pump does.
            val pump =
                scope.launch {
                    while (true) {
                        Snapshot.sendApplyNotifications()
                        delay(5)
                    }
                }
            try {
                fresh = false
                assertFalse(withTimeout(5_000) { startsFresh(AddonsView(client, scope).also { it.open() }) })
                fresh = true
                assertTrue(withTimeout(5_000) { startsFresh(AddonsView(client, scope).also { it.open() }) })
            } finally {
                pump.cancel()
            }
        }
}
