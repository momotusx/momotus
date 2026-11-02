package app.marquee.core

import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleState
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** What a kids session reads (plans/m11-kids.md 6.4). */
class KidsCopyTest {
    private fun header(state: TitleState) = TitleHeader(state = state, inLibrary = false, watched = false)

    @Test
    fun `a title off the list says so at once, never Loading (QA round 1, Q7 row 28)`() {
        assertEquals(
            "That isn't on your list" to "Ask a grown-up.",
            KidsCopy.titleRefusal(header(TitleState.NotAllowed)),
        )
        assertNull(KidsCopy.titleRefusal(header(TitleState.Loading)))
        assertNull(KidsCopy.titleRefusal(header(TitleState.Ready)))
        assertNull(KidsCopy.titleRefusal(null))
    }
}
