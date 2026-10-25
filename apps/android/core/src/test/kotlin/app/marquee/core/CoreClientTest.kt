package app.marquee.core

import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.client.RepeatingReplay
import app.marquee.core.client.ReplayTransport
import app.marquee.core.client.Trace
import app.marquee.core.client.ViewBinding
import app.marquee.core.client.ViewSink
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.HomeUpdate
import app.marquee.core.model.RowState
import app.marquee.core.model.applyDiffs
import app.marquee.core.session.CoreOps
import app.marquee.core.session.LifecycleEvent
import app.marquee.core.views.HomeView
import app.marquee.core.views.SearchView
import app.marquee.core.views.TitleView
import app.marquee.core.views.ViewStatus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds

class CoreClientTest {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val client = CoreClient(executor.asCoroutineDispatcher())

    @AfterTest
    fun stop() {
        client.close()
        executor.shutdownNow()
    }

    private fun trace(name: String) = Trace.parse(javaClass.getResource("/traces/$name.jsonl")!!.readText())

    private suspend fun until(
        what: String,
        check: () -> Boolean,
    ) {
        withTimeout(5_000) {
            while (!check()) delay(5)
        }
        assertTrue(check(), what)
    }

    private fun requestOf(text: String): JsonObject = CoreJson.parseToJsonElement(text).jsonObject

    private fun idOf(text: String) = requestOf(text).getValue("id").jsonPrimitive.long

    private fun opOf(text: String) = requestOf(text)["op"]?.jsonPrimitive?.content

    @Test
    fun `a recorded Home trace ends in the state the diffs describe`() =
        runBlocking {
            val recorded = trace("home")
            // Reference: fold the recorded snapshot and diffs with the plain list semantics.
            val snapshot =
                recorded
                    .first { it.msg["result"]?.jsonObject?.get("snapshot") != null }
                    .msg
                    .getValue("result")
                    .jsonObject
                    .getValue("snapshot")
            val expected = CoreJson.decodeFromJsonElement(HomeSnapshot.serializer(), snapshot).rows.toMutableList()
            for (line in recorded) {
                val updates = line.msg["updates"] as? JsonArray ?: continue
                for (u in updates) {
                    val update = CoreJson.decodeFromJsonElement(HomeUpdate.serializer(), u)
                    if (update is HomeUpdate.Rows) expected.applyDiffs(update.diffs)
                }
            }
            assertTrue(expected.any { it.state is RowState.Ready }, "the trace must contain loaded rows")

            client.attach(ReplayTransport(recorded) { client.onMessage(it) })
            val view = HomeView(client, scope).also { it.open() }
            view.opened.await()
            until("every recorded update applied") { view.rows.toList() == expected }
            assertEquals(ViewStatus.Open, view.status)
            view.close()
        }

    @Test
    fun `a failed Home row's reason goes to the log once, without its address`() =
        runBlocking {
            val logs = CopyOnWriteArrayList<String>()
            app.marquee.core.client.CoreLog.sink = { level, message, _ -> logs += "$level $message" }
            val address = "https://addon.example/k3y/catalog/movie/top.json"
            val failed = """{"state":"failed","reason":"network: error sending request ($address)"}"""

            fun row(state: String) =
                """{"rowId":"a@1:movie:top","source":"A","state":$state,"tiles":[],"title":"Popular Movies"}"""

            fun push(state: String) =
                client.onMessage(
                    """{"sub":5,"updates":[{"kind":"rows","diffs":[{"index":0,"op":"set","value":${row(state)}}]}]}""",
                )
            client.attach { text ->
                if (opOf(text) == "subscribe") {
                    client.onMessage(
                        """{"id":${idOf(
                            text,
                        )},"ok":true,"result":{"sub":5,"snapshot":{"header":{"hero":null},"rows":[${row(failed)}]}}}""",
                    )
                }
            }
            try {
                val view = HomeView(client, scope).also { it.open() }
                view.opened.await()
                until("the failure is logged") { logs.isNotEmpty() }
                assertEquals(
                    listOf("w home row a@1:movie:top did not load: network: error sending request (<address>)"),
                    logs.toList(),
                )
                // Still failed: not again. Loaded, then failed anew: once more.
                push(failed)
                push("""{"state":"loading"}""")
                until("the row loads again") { view.rows.single().state == RowState.Loading }
                push(failed)
                until("the new failure is logged") { logs.size == 2 }
                push(failed)
                delay(200)
                assertEquals(2, logs.size, logs.toString())
                view.close()
            } finally {
                app.marquee.core.client.CoreLog.sink = { _, _, _ -> }
            }
        }

    @Test
    fun `replaying the same trace again and again with new subscription ids keeps working`() =
        runBlocking {
            val recorded = trace("home")
            client.attach(RepeatingReplay(recorded) { client.onMessage(it) })
            repeat(5) {
                val view = HomeView(client, scope).also { it.open() }
                view.opened.await()
                until("rows") { view.rows.size == 6 }
                view.close()
            }
        }

    @Test
    fun `a subscribe reply without a subscription id fails that subscribe only`() =
        runBlocking {
            client.attach(
                CoreTransport { text ->
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{}}""")
                },
            )
            val view = HomeView(client, scope).also { it.open() }
            until("failed") { view.status is ViewStatus.Failed }
        }

    @Test
    fun `a title view keeps header and episodes and sends commands`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<String>()
            val replay = ReplayTransport(trace("title-series")) { client.onMessage(it) }
            client.attach { text ->
                sent += text
                replay.send(text)
            }
            val view = TitleView(client, scope, "series", "tt0944947").also { it.open() }
            view.opened.await()
            until("episodes loaded") { view.episodes.isNotEmpty() }
            view.selectSeason(2)
            until("command went out") { sent.any { it.contains("selectSeason") } }
            view.close()
        }

    @Test
    fun `a title view tells a fresh subscription it shows again after Try again`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<String>()
            var subs = 0
            client.attach { text ->
                sent += text
                if (opOf(text) == "subscribe") {
                    subs++
                    val page =
                        """{"header":{"state":{"state":"loading"},"inLibrary":false,"watched":false},"episodes":[]}"""
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{"sub":$subs,"snapshot":$page}}""")
                } else if (opOf(text) != "command") {
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{}}""")
                }
            }
            val view = TitleView(client, scope, "movie", "tt0111161").also { it.open() }
            view.opened.await()
            view.setVisible(true)
            val visible = { sub: Int ->
                sent.count {
                    val m = requestOf(it)
                    opOf(it) == "command" && m["sub"]?.jsonPrimitive?.long == sub.toLong() &&
                        it.contains("\"titleVisible\"")
                }
            }
            until("visible told to the first subscription") { visible(1) == 1 }
            view.retry()
            // The core starts a new subscription hidden: without this, TMDB and Trakt stay pending.
            until("visible told to the fresh subscription") { visible(2) == 1 }
            view.close()
        }

    @Test
    fun `commands sent before the subscription is open wait and go out in order`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<String>()
            val gate = CountDownLatch(1)
            client.attach { text ->
                sent += text
                if (opOf(text) == "subscribe") {
                    // The core is slow to answer the subscribe.
                    Thread {
                        gate.await()
                        client.onMessage(
                            """{"id":${idOf(
                                text,
                            )},"ok":true,"result":{"sub":5,"snapshot":{"query":"","rows":[],"recent":[]}}}""",
                        )
                    }.start()
                }
            }
            val search = SearchView(client, scope).also { it.open() }
            search.setQuery("sin")
            search.setQuery("sintel")
            delay(100)
            assertEquals(1, sent.size, "only the subscribe has gone out")
            gate.countDown()
            until("both commands sent") { sent.size == 3 }
            assertTrue(sent[1].contains("\"query\":\"sin\"") && sent[2].contains("\"query\":\"sintel\""))
            search.close()
        }

    @Test
    fun `a push that beats its subscribe reply is kept and applied`() =
        runBlocking {
            val seen = CopyOnWriteArrayList<String>()
            val sink =
                object : ViewSink<String, String> {
                    override fun onSnapshot(snapshot: String) {
                        seen += "snapshot:$snapshot"
                    }

                    override fun onUpdates(updates: List<String>) {
                        seen += "updates:${updates.joinToString()}"
                    }

                    override fun onFailure(reason: String) {
                        seen += "failure:$reason"
                    }
                }
            client.attach(
                CoreTransport { text ->
                    client.onMessage("""{"sub":7,"updates":["early"]}""")
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{"sub":7,"snapshot":"first"}}""")
                },
            )
            client.subscribe("x", JsonObject(emptyMap()), ViewBinding(String.serializer(), String.serializer(), sink))
            until("both applied") { seen.size == 2 }
            assertEquals(listOf("snapshot:first", "updates:early"), seen)
        }

    @Test
    fun `pushes that reach the state thread together are applied as one batch per view`() =
        runBlocking {
            val batches = CopyOnWriteArrayList<List<String>>()
            val sink =
                object : ViewSink<String, String> {
                    override fun onSnapshot(snapshot: String) = Unit

                    override fun onUpdates(updates: List<String>) {
                        batches += updates.toList()
                    }

                    override fun onFailure(reason: String) = Unit
                }
            client.attach(
                CoreTransport { text ->
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{"sub":3,"snapshot":"s"}}""")
                },
            )
            client.subscribe("x", JsonObject(emptyMap()), ViewBinding(String.serializer(), String.serializer(), sink))

            // Hold the state thread, queue five pushes, release: they must arrive as one call.
            val gate = CountDownLatch(1)
            executor.submit { gate.await() }
            repeat(5) { client.onMessage("""{"sub":3,"updates":["u$it"]}""") }
            gate.countDown()
            until("applied") { batches.sumOf { it.size } == 5 }
            assertEquals(listOf(listOf("u0", "u1", "u2", "u3", "u4")), batches.toList())
        }

    @Test
    fun `an ok false reply becomes a CoreError with the core's text`() =
        runBlocking {
            client.attach(
                CoreTransport { text ->
                    client.onMessage("""{"id":${idOf(text)},"ok":false,"error":"unknown view: nope"}""")
                },
            )
            val error =
                assertFailsWith<CoreError> { client.request("subscribe", mapOf("view" to JsonPrimitive("nope"))) }
            assertEquals("unknown view: nope", error.message)
        }

    @Test
    fun `an error reply keeps its code, so an app can act on it`() =
        runBlocking {
            client.attach(
                CoreTransport { text ->
                    client.onMessage("""{"id":${idOf(text)},"ok":false,"error":"removed","code":"dropped"}""")
                },
            )
            val error = assertFailsWith<CoreError> { client.request("syncNow") }
            assertEquals("dropped", error.code)
        }

    @Test
    fun `replies may come in any order`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<Long>()
            client.attach(CoreTransport { text -> sent += idOf(text) })
            val first = async { client.request("installAddon") }
            val second = async { client.request("setPrefs") }
            until("both sent") { sent.size == 2 }
            // The second answers first.
            client.onMessage("""{"id":${sent[1]},"ok":true,"result":{"which":"second"}}""")
            client.onMessage("""{"id":${sent[0]},"ok":true,"result":{"which":"first"}}""")
            val a = first.await()
            val b = second.await()
            assertEquals(
                "first",
                a.jsonObject
                    .getValue("which")
                    .jsonPrimitive.content,
            )
            assertEquals(
                "second",
                b.jsonObject
                    .getValue("which")
                    .jsonPrimitive.content,
            )
        }

    @Test
    fun `a request the core never answers times out`() =
        runBlocking {
            client.attach(CoreTransport { })
            val error = assertFailsWith<CoreError> { client.request("setPrefs", timeout = 50.milliseconds) }
            assertTrue(error.timedOut)
        }

    @Test
    fun `requests made before the core is attached wait for it without starting their timer`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<Long>()
            val waiting = async { client.request("setPrefs", timeout = 100.milliseconds) }
            delay(300) // longer than the timeout: the boot is slow, the request must not give up
            client.attach(
                CoreTransport { text ->
                    sent += idOf(text)
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{}}""")
                },
            )
            waiting.await()
            assertEquals(1, sent.size)
        }

    @Test
    fun `a core that could not start fails waiting and later requests with the reason`() =
        runBlocking {
            val waiting = async { runCatching { client.request("setPrefs") } }
            delay(50)
            client.attachFailed("The core could not start: no library")
            val error = waiting.await().exceptionOrNull()
            assertEquals("The core could not start: no library", error?.message)
            assertFailsWith<CoreError> { client.request("setPrefs") }
            Unit
        }

    @Test
    fun `a diff that does not fit makes the view subscribe again, once`() =
        runBlocking {
            val subscribed = CopyOnWriteArrayList<Long>()
            val unsubscribed = CopyOnWriteArrayList<Long>()
            client.attach(
                CoreTransport { text ->
                    val request = requestOf(text)
                    val id = request.getValue("id").jsonPrimitive.long
                    when (request["op"]?.jsonPrimitive?.content) {
                        "subscribe" -> {
                            val sub = 10L + subscribed.size
                            subscribed += sub
                            client.onMessage(
                                """{"id":$id,"ok":true,"result":{"sub":$sub,"snapshot":{"header":{"hero":null},"rows":[]}}}""",
                            )
                            if (subscribed.size == 1) {
                                // A bad diff in the very first batch, the case that used to race the assignment.
                                client.onMessage(
                                    """{"sub":$sub,"updates":[{"kind":"rows","diffs":[{"op":"remove","index":4}]}]}""",
                                )
                            }
                        }

                        "unsubscribe" -> {
                            unsubscribed += request.getValue("sub").jsonPrimitive.long
                            client.onMessage("""{"id":$id,"ok":true,"result":{}}""")
                        }
                    }
                },
            )
            val view = HomeView(client, scope).also { it.open() }
            until("subscribed twice") { subscribed.size == 2 }
            until("the first subscription was closed") { unsubscribed.contains(10L) }
            until("open again") { view.status == ViewStatus.Open }
            assertEquals(listOf(10L, 11L), subscribed.toList())
            assertEquals(0, view.rows.size)
        }

    private fun homeCore(
        subscribed: MutableList<Long>,
        unsubscribed: MutableList<Long>,
        firstSub: Long,
    ) = CoreTransport { text ->
        val request = requestOf(text)
        val id = request.getValue("id").jsonPrimitive.long
        when (request["op"]?.jsonPrimitive?.content) {
            "subscribe" -> {
                subscribed += firstSub
                client.onMessage(
                    """{"id":$id,"ok":true,"result":{"sub":$firstSub,"snapshot":{"header":{"hero":null},"rows":[]}}}""",
                )
            }

            "unsubscribe" -> {
                unsubscribed += request.getValue("sub").jsonPrimitive.long
            }
        }
        // any other request (setPrefs in these tests) is never answered
    }

    @Test
    fun `lifecycle events and ping go out with the names the core expects`() =
        runBlocking {
            val sent = CopyOnWriteArrayList<JsonObject>()
            client.attach(
                CoreTransport { text ->
                    sent += requestOf(text)
                    client.onMessage("""{"id":${idOf(text)},"ok":true,"result":{}}""")
                },
            )
            val ops = CoreOps(client)
            ops.ping()
            ops.lifecycle(LifecycleEvent.MemoryPressure)
            until("both sent") { sent.size == 2 }
            assertEquals("ping", sent[0].getValue("op").jsonPrimitive.content)
            assertEquals("lifecycle", sent[1].getValue("op").jsonPrimitive.content)
            assertEquals("memoryPressure", sent[1].getValue("event").jsonPrimitive.content)
        }

    @Test
    fun `a core that died fails what waits for it and refuses new requests`() =
        runBlocking {
            val died = CopyOnWriteArrayList<String>()
            client.onCoreDied = { died += it }
            client.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))
            val view = HomeView(client, scope).also { it.open() }
            view.opened.await()
            until("open") { view.status == ViewStatus.Open }
            val waiting = async { runCatching { client.request("setPrefs") } }
            delay(50)

            client.onMessage("""{"core":"died","reason":"task panicked"}""")

            assertEquals("The core stopped.", waiting.await().exceptionOrNull()?.message)
            until("reported") { died == listOf("task panicked") }
            until("the view shows it") { view.status == ViewStatus.Failed("The core stopped.") }
            assertFailsWith<CoreError> { client.request("setPrefs") }
            assertTrue(view.currentSubscription?.alive == false)
            Unit
        }

    @Test
    fun `an app that stalls past its inbox budget ends that core like an overflow`() =
        runBlocking {
            val stalled = Executors.newSingleThreadExecutor { Thread(it, "stalled-state") }
            val small = CoreClient(stalled.asCoroutineDispatcher(), inboxBudget = 1_000)
            try {
                val died = CopyOnWriteArrayList<String>()
                small.onCoreDied = { died += it }
                small.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))
                // The state thread is busy: nothing leaves the inbox while the core keeps sending.
                val gate = java.util.concurrent.CountDownLatch(1)
                stalled.submit { gate.await() }
                val push = """{"sub":99,"updates":[${"0,".repeat(40)}0]}"""
                repeat(100) { small.onMessage(push) }
                gate.countDown()
                until("reported") { died.singleOrNull()?.startsWith("overflow") == true }
                Unit
            } finally {
                small.close()
                stalled.shutdownNow()
            }
        }

    @Test
    fun `an overflowed core is opened again like a dead one`() =
        runBlocking {
            val died = CopyOnWriteArrayList<String>()
            client.onCoreDied = { died += it }
            client.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))
            val view = HomeView(client, scope).also { it.open() }
            view.opened.await()

            client.onMessage("""{"core":"overflow"}""")

            until("reported") { died.singleOrNull()?.startsWith("overflow") == true }
            until("the view shows it") { view.status == ViewStatus.Failed("The core stopped.") }
            assertFailsWith<CoreError> { client.request("setPrefs") }
            Unit
        }

    @Test
    fun `a view that failed because the core never started comes back when one is attached`() =
        runBlocking {
            client.attachFailed("The core could not start: no library")
            val view = HomeView(client, scope).also { it.open() }
            until("failed") { view.status is ViewStatus.Failed }

            client.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))

            until("open") { view.status == ViewStatus.Open }
        }

    @Test
    fun `pushes of the dead core never reach a view of the new one`() =
        runBlocking {
            client.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))
            val view = HomeView(client, scope).also { it.open() }
            until("open") { view.status == ViewStatus.Open }
            client.onMessage("""{"core":"died","reason":"x"}""")
            // The dead core's last words, with the id the new core is about to reuse.
            client.onMessage("""{"sub":10,"updates":[{"kind":"rows","diffs":[{"op":"remove","index":0}]}]}""")
            until("failed") { view.status is ViewStatus.Failed }

            client.attach(homeCore(CopyOnWriteArrayList(), CopyOnWriteArrayList(), 10))

            until("open again") { view.status == ViewStatus.Open }
            delay(100)
            assertEquals(ViewStatus.Open, view.status) // the stale remove did not make it resubscribe in a loop
            assertEquals(0, view.rows.size)
        }

    @Test
    fun `views of a dead core subscribe again on the new one and never close its subscriptions`() =
        runBlocking {
            val subscribed = CopyOnWriteArrayList<Long>()
            val unsubscribed = CopyOnWriteArrayList<Long>()
            client.attach(homeCore(subscribed, unsubscribed, 10))
            val view = HomeView(client, scope).also { it.open() }
            until("open") { view.status == ViewStatus.Open }
            client.onMessage("""{"core":"died","reason":"x"}""")
            until("failed") { view.status is ViewStatus.Failed }

            // A new core hands out the same id for its first view: it must survive the old view's cleanup.
            client.attach(homeCore(subscribed, unsubscribed, 10))
            until("subscribed again") { subscribed.size == 2 }
            until("open again") { view.status == ViewStatus.Open }
            delay(100)
            assertEquals(emptyList<Long>(), unsubscribed.toList())
            assertTrue(view.currentSubscription?.alive == true)
            view.close()
            until("closed on the new core") { unsubscribed == listOf(10L) }
        }

    @Test
    fun `a subscription that opens after its caller gave up is closed, not leaked`() =
        runBlocking {
            val unsubscribed = CopyOnWriteArrayList<Long>()
            val held = CopyOnWriteArrayList<Long>()
            client.attach(
                CoreTransport { text ->
                    val request = requestOf(text)
                    val id = request.getValue("id").jsonPrimitive.long
                    when (request["op"]?.jsonPrimitive?.content) {
                        "subscribe" -> held += id

                        // the core is busy: the reply comes much later
                        "unsubscribe" -> unsubscribed += request.getValue("sub").jsonPrimitive.long
                    }
                },
            )
            val view = HomeView(client, scope).also { it.open() }
            until("subscribe sent") { held.size == 1 }
            view.close() // the screen left before the core answered
            delay(50)
            client.onMessage(
                """{"id":${held[0]},"ok":true,"result":{"sub":42,"snapshot":{"header":{"hero":null},"rows":[]}}}""",
            )
            until("the orphan was closed") { unsubscribed.contains(42L) }
        }

    @Test
    fun `closing a view before it opened ends anyone waiting for it`() =
        runBlocking {
            client.attach(CoreTransport { })
            val view = HomeView(client, scope).also { it.open() }
            val waiter = async { runCatching { view.opened.await() } }
            delay(50)
            view.close()
            assertTrue(waiter.await().isFailure)
        }
}
