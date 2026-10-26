package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createComposeRule
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.stateThread
import app.marquee.core.views.TitleView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.Collections
import kotlin.test.assertEquals

/**
 * The title page tells the core it shows as soon as it is on screen, and that it stopped when it goes
 * (plans/title-page-ratings-genres-similar.md, `titleVisible`): a core that never hears it keeps TMDB's and Trakt's
 * slots pending.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TitleVisibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = stateThread("test-state")
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `visible goes when the page first shows, and hidden when it leaves`() {
        val sent = Collections.synchronizedList(mutableListOf<Boolean>())
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val command = msg["command"]?.jsonObject
            if (command?.get("type")?.jsonPrimitive?.content == "titleVisible") {
                sent += command["visible"]!!.jsonPrimitive.content.toBoolean()
            }
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val subscribe = msg["op"]?.jsonPrimitive?.content == "subscribe"
            val result = if (subscribe) """{"sub":7,"snapshot":$PAGE}""" else "{}"
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt0111161")
        var shown by mutableStateOf(true)
        compose.setContent { if (shown) TitleVisibility(view) }
        compose.waitForIdle()
        view.open()
        waitFor { sent.isNotEmpty() }
        assertEquals(listOf(true), sent.toList())
        shown = false
        compose.waitForIdle()
        waitFor { sent.size == 2 }
        assertEquals(listOf(true, false), sent.toList())
    }

    private companion object {
        val PAGE = """{"header":{"state":{"state":"loading"},"inLibrary":false,"watched":false},"episodes":[]}"""
    }

    private fun waitFor(done: () -> Boolean) {
        val deadline = System.currentTimeMillis() + 30_000
        while (!done()) {
            check(System.currentTimeMillis() < deadline) { "titleVisible never went to the core" }
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }
}
