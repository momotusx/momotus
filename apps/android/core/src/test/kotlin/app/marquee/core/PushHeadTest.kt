package app.marquee.core

import app.marquee.core.client.PushHead
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class PushHeadTest {
    @Test
    fun `reads the subscription id and where the updates start`() {
        val text = """{"sub":42,"updates":[{"kind":"rows"}]}"""
        val head = PushHead.scan(text)!!
        assertEquals(42L, head.sub)
        assertEquals("""[{"kind":"rows"}]""", text.substring(head.updatesStart, text.length - 1))
    }

    @Test
    fun `anything else falls back to the tree`() {
        assertNull(PushHead.scan("""{"id":1,"ok":true,"result":{}}"""))
        assertNull(PushHead.scan("""{"updates":[],"sub":1}"""))
        assertNull(PushHead.scan("""{"sub":,"updates":[]}"""))
        assertNull(PushHead.scan("""{"sub":7,"updates":[]"""))
    }
}
