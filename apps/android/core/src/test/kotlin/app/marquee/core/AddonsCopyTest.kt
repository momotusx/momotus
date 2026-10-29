package app.marquee.core

import app.marquee.core.client.CoreJson
import app.marquee.core.model.AddonInfo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

class AddonsCopyTest {
    private val json =
        """{"url":"https://tta.example/manifest.json","host":"tta.example","name":"TurkceAltyazi.org",""" +
            """"version":"1.1.1","enabled":true,"configurable":false"""

    @Test
    fun `a built-in row says where it runs, never a host`() {
        val builtIn = CoreJson.decodeFromString(AddonInfo.serializer(), "$json,\"builtIn\":true}")
        assertEquals("Built into Momotus, runs on this device · 1.1.1", AddonsCopy.caption(builtIn))
        assertFalse("tta.example" in AddonsCopy.caption(builtIn))
        // Any other row names its host.
        assertEquals("tta.example · 1.1.1", AddonsCopy.caption(builtIn.copy(builtIn = false)))
    }

    @Test
    fun `a core without the field decodes every row as an addon of its own`() {
        assertFalse(CoreJson.decodeFromString(AddonInfo.serializer(), "$json}").builtIn)
    }

    @Test
    fun `Settings' Addons row counts the addons and the ones on that find streams, naming none`() {
        val row = CoreJson.decodeFromString(AddonInfo.serializer(), "$json}")
        val streams = row.copy(name = "Streamer", provides = listOf("stream"))
        assertEquals("None installed yet.", AddonsCopy.summary(emptyList()))
        assertEquals("1 installed. None of them finds streams yet.", AddonsCopy.summary(listOf(row)))
        // One that is off finds nothing.
        assertEquals(
            "3 installed. 1 finds streams.",
            AddonsCopy.summary(listOf(row, streams, streams.copy(enabled = false))),
        )
        val many = AddonsCopy.summary(listOf(streams, streams, row))
        assertEquals("3 installed. 2 find streams.", many)
        assertFalse("Streamer" in many)
    }
}
