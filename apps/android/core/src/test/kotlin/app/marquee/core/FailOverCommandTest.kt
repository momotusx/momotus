package app.marquee.core

import app.marquee.core.views.failOverCommand
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals

class FailOverCommandTest {
    private fun expected(vararg fields: Pair<String, JsonPrimitive>) =
        JsonObject(mapOf("type" to JsonPrimitive("failOver")) + fields)

    @Test
    fun `a failover names the attempt of the source the player loaded and what failed`() {
        assertEquals(
            expected(
                "error" to JsonPrimitive("The source answered with HTTP 404"),
                "kind" to JsonPrimitive("http"),
                "status" to JsonPrimitive(404),
                "attempt" to JsonPrimitive(3L),
            ),
            failOverCommand("The source answered with HTTP 404", 3L, "http", 404),
        )
    }

    @Test
    fun `an unknown attempt or status is left out, so the core acts on the current source`() {
        assertEquals(
            expected("error" to JsonPrimitive("decoder error"), "kind" to JsonPrimitive("decode")),
            failOverCommand("decoder error", null, "decode", null),
        )
    }
}
