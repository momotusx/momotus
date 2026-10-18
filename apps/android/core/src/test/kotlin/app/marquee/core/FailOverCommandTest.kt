package app.marquee.core

import app.marquee.core.views.failOverCommand
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals

class FailOverCommandTest {
    @Test
    fun `a failover names the attempt of the source the player loaded`() {
        val expected =
            JsonObject(
                mapOf(
                    "type" to JsonPrimitive("failOver"),
                    "error" to JsonPrimitive("decoder error"),
                    "attempt" to JsonPrimitive(3L),
                ),
            )
        assertEquals(expected, failOverCommand("decoder error", 3L))
    }

    @Test
    fun `an unknown attempt is left out, so the core acts on the current source`() {
        val expected = JsonObject(mapOf("type" to JsonPrimitive("failOver"), "error" to JsonPrimitive("decoder error")))
        assertEquals(expected, failOverCommand("decoder error", null))
    }
}
