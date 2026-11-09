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
import kotlin.test.assertNotNull
import kotlin.test.assertNull

/**
 * First run's still (`AppState.askStill`): the core's none (a phone's first requests past its budget) is not kept,
 * so the next framed step asks again and the art comes late instead of never; a still, once it came, is kept.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class FirstRunStillAskTest {
    private val caps: CapsSource = { error("no Versions view opens here") }

    @Test
    fun `no still is asked again by the next step, and a still that came is kept`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        val asks = AtomicInteger()
        val answered = AtomicInteger()
        session.client.attach(
            CoreTransport { text ->
                val request = CoreJson.parseToJsonElement(text).jsonObject
                if (request.getValue("op").jsonPrimitive.content != "firstRunStill") return@CoreTransport
                val id = request.getValue("id").jsonPrimitive.long
                // The first ask past the budget, the second in time.
                val result =
                    if (asks.incrementAndGet() == 1) {
                        "null"
                    } else {
                        """{"background":"https://art.example/b.jpg","name":"B"}"""
                    }
                session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
                answered.incrementAndGet()
            },
        )
        val state = AppState(session, caps, shutsBefore = 0)

        // The answer comes on the client's thread; the state takes it on the main looper.
        fun settle(after: Int) {
            repeat(500) {
                shadowOf(Looper.getMainLooper()).idle()
                if (answered.get() >= after) {
                    repeat(20) {
                        Thread.sleep(5)
                        shadowOf(Looper.getMainLooper()).idle()
                    }
                    return
                }
                Thread.sleep(5)
            }
        }

        state.askStill()
        settle(1)
        assertNull(state.still)
        // The next framed step.
        state.askStill()
        settle(2)
        assertNotNull(state.still)
        assertEquals("B", state.still?.name)
        // Every step after keeps it: no third ask.
        state.askStill()
        settle(3)
        assertEquals(2, asks.get())
    }
}
