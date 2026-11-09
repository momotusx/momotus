package app.marquee.android

import android.os.Looper
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.session.CoreSession
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.assertEquals

/**
 * First run's wall (`AppState.askWall`, plans/first-screens.md 3.1): asked as soon as the start is first run and never
 * on an ordinary start; an empty answer (past the core's budget, offline) is asked again by the next step, and a list
 * that came is kept.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class FirstRunWallAskTest {
    private val caps: CapsSource = { error("no Versions view opens here") }

    /** A core whose addons view says [fresh]; the wall's answers are [walls] in turn. */
    private fun start(
        fresh: Boolean,
        walls: List<String>,
        asks: AtomicInteger,
    ): Pair<CoreSession, AppState> {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        session.client.attach(
            CoreTransport { text ->
                val request = CoreJson.parseToJsonElement(text).jsonObject
                val id = request["id"]?.jsonPrimitive?.long ?: return@CoreTransport
                val result =
                    when (request.getValue("op").jsonPrimitive.content) {
                        "firstRunWall" -> walls[minOf(asks.getAndIncrement(), walls.lastIndex)]
                        "subscribe" -> """{"sub":7,"snapshot":{"addons":[],"fresh":$fresh}}"""
                        else -> "{}"
                    }
                session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
            },
        )
        session.opened(running = true)
        return session to AppState(session, caps, shutsBefore = 0)
    }

    private fun settle() =
        repeat(200) {
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(5)
        }

    @Test
    fun `an ordinary start never asks for the wall`() {
        val asks = AtomicInteger()
        val (_, state) = start(fresh = false, walls = listOf("[]"), asks)
        settle()
        assertEquals(Start.Home, state.start)
        assertEquals(0, asks.get())
    }

    @Test
    fun `a first run asks at once, again after an empty answer, and keeps a list that came`() {
        val asks = AtomicInteger()
        val (_, state) = start(fresh = true, walls = listOf("[]", """["https://art.example/a.jpg"]"""), asks)
        settle()
        assertEquals(Start.Onboarding, state.start)
        assertEquals(1, asks.get())
        assertEquals(emptyList(), state.wall)
        // The next step asks again.
        state.askWall()
        settle()
        assertEquals(listOf("https://art.example/a.jpg"), state.wall)
        // Every step after keeps it: no third ask.
        state.askWall()
        settle()
        assertEquals(2, asks.get())
    }
}
