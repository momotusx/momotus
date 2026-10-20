package app.marquee.player

import androidx.compose.ui.geometry.Rect
import org.junit.Test
import kotlin.test.assertEquals

/** Where the phone's cues go beside a control in the bottom corner, on a 900 by 400 screen. */
class CueFrameTest {
    private val text = 0.05f
    private val foot = 300f

    private fun frame(corner: Rect?) =
        cueFrame(height = 400f, width = 900f, foot = foot, ceiling = 0f, text = text, corner = corner)

    @Test
    fun `beside a narrow corner control the lines stay centred, short of it`() {
        val skip = Rect(left = 720f, top = 240f, right = 884f, bottom = 320f)
        val frame = frame(skip)
        assertEquals(0.5f, frame.center)
        assertEquals(720f, 900f * (frame.center + frame.width / 2), 1e-3f)
    }

    @Test
    fun `beside a wide one they take the room left of it, centred there`() {
        val upNext = Rect(left = 520f, top = 150f, right = 884f, bottom = 320f)
        val frame = frame(upNext)
        assertEquals(520f / 900f, frame.width, 1e-6f)
        assertEquals(260f / 900f, frame.center, 1e-6f)
    }

    @Test
    fun `a corner control under the lines or over two of them leaves them the whole width`() {
        assertEquals(1f, frame(Rect(left = 520f, top = foot, right = 884f, bottom = 380f)).width)
        assertEquals(1f, frame(Rect(left = 520f, top = 0f, right = 884f, bottom = 200f)).width)
    }
}
