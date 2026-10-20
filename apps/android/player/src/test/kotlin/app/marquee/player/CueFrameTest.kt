package app.marquee.player

import android.graphics.Bitmap
import androidx.compose.ui.geometry.Rect
import androidx.media3.common.text.Cue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** Where the phone's cues go beside a control in the bottom corner, on a 900 by 400 screen, and which cues the lift moves. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
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

    private val lifted = CueFrame(bottom = 0.5f, text = text)

    @Test
    fun `while lifted, a cue on a bottom line ends at the foot, one stacked over it a line higher`() {
        val bottomLine =
            Cue
                .Builder()
                .setText("a")
                .setLine(-1f, Cue.LINE_TYPE_NUMBER)
                .build()
        val fitted = lifted.fit(bottomLine, lifted = true)
        assertEquals(Cue.LINE_TYPE_FRACTION, fitted.lineType)
        assertEquals(Cue.ANCHOR_TYPE_END, fitted.lineAnchor)
        assertEquals(0.5f, fitted.line, 1e-6f)
        val stacked = lifted.fit(bottomLine.buildUpon().setLine(-2f, Cue.LINE_TYPE_NUMBER).build(), lifted = true)
        assertEquals(0.5f - CUE_LINE * text, stacked.line, 1e-6f)
        assertEquals(0.5f, lifted.fit(Cue.Builder().setText("a").build(), lifted = true).line, 1e-6f)
    }

    @Test
    fun `a picture cue, a cue on a line of its own, or any cue unlifted stays where the file put it`() {
        val picture =
            Cue
                .Builder()
                .setBitmap(
                    Bitmap.createBitmap(4, 2, Bitmap.Config.ARGB_8888),
                ).setLine(0.8f, Cue.LINE_TYPE_FRACTION)
                .build()
        val placed =
            Cue
                .Builder()
                .setText("a")
                .setLine(0.1f, Cue.LINE_TYPE_FRACTION)
                .build()
        val fromTop =
            Cue
                .Builder()
                .setText("a")
                .setLine(2f, Cue.LINE_TYPE_NUMBER)
                .build()
        val bottomLine =
            Cue
                .Builder()
                .setText("a")
                .setLine(-1f, Cue.LINE_TYPE_NUMBER)
                .build()
        for (cue in listOf(picture, placed, fromTop)) assertEquals(cue.line, lifted.fit(cue, lifted = true).line)
        assertEquals(-1f, lifted.fit(bottomLine, lifted = false).line)
    }
}
