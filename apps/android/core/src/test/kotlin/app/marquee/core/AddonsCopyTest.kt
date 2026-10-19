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
        assertEquals("Built into Marquee, runs on this device · 1.1.1", AddonsCopy.caption(builtIn))
        assertFalse("tta.example" in AddonsCopy.caption(builtIn))
        // Any other row names its host.
        assertEquals("tta.example · 1.1.1", AddonsCopy.caption(builtIn.copy(builtIn = false)))
    }

    @Test
    fun `a core without the field decodes every row as an addon of its own`() {
        assertFalse(CoreJson.decodeFromString(AddonInfo.serializer(), "$json}").builtIn)
    }

    @Test
    fun `About credits TurkceAltyazi as the plan words it`() {
        assertEquals(
            "Turkish subtitles from TurkceAltyazi.org, fetched by this device when you play.",
            AddonsCopy.TURKCEALTYAZI_CREDIT,
        )
    }
}
