package app.marquee.player

import android.graphics.Bitmap
import androidx.compose.ui.geometry.Rect
import androidx.media3.common.text.Cue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Where the phone's cues go beside a control in the bottom corner, on a 900 by 400 screen, and which cues hang from the foot. */
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

    /** The controls hidden: the foot at Media3's own bottom padding, 8% over the screen's. */
    private val plain = CueFrame(bottom = 0.08f, text = text)

    @Test
    fun `while lifted, a cue on a bottom line ends at the foot, one stacked over it a line higher`() {
        val bottomLine =
            Cue
                .Builder()
                .setText("a")
                .setLine(-1f, Cue.LINE_TYPE_NUMBER)
                .build()
        val fitted = lifted.fit(bottomLine)
        assertEquals(Cue.LINE_TYPE_FRACTION, fitted.lineType)
        assertEquals(Cue.ANCHOR_TYPE_END, fitted.lineAnchor)
        assertEquals(0.5f, fitted.line, 1e-6f)
        val stacked = lifted.fit(bottomLine.buildUpon().setLine(-2f, Cue.LINE_TYPE_NUMBER).build())
        assertEquals(0.5f - CUE_LINE * text, stacked.line, 1e-6f)
        assertEquals(0.5f, lifted.fit(Cue.Builder().setText("a").build()).line, 1e-6f)
    }

    @Test
    fun `a picture cue or a cue on a line of its own stays where the file put it`() {
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
        for (cue in listOf(picture, placed, fromTop)) {
            assertEquals(cue.line, lifted.fit(cue).line)
            assertEquals(cue.line, plain.fit(cue).line)
        }
    }

    @Test
    fun `with the controls hidden two lines on the bottom line hang from the foot, over the safe line, growing up`() {
        // A WebVTT bottom line (every addon line): Media3 draws line -1 on the view's last pixel.
        val twoLines =
            Cue
                .Builder()
                .setText("MI5 SECURITY SERVICE\nSECRET")
                .setLine(-1f, Cue.LINE_TYPE_NUMBER)
                .build()
        val fitted = plain.fit(twoLines)
        assertEquals(Cue.LINE_TYPE_FRACTION, fitted.lineType)
        assertEquals(Cue.ANCHOR_TYPE_END, fitted.lineAnchor, "anchored at its foot: the second line grows upward")
        assertEquals(0.92f, fitted.line, 1e-6f)
        // Both lines' box, as fractions of the screen, between the top and the 5% title-safe line.
        val top = fitted.line - 2 * CUE_LINE * text
        assertTrue(fitted.line <= 0.95f && top >= 0f, "the box $top..${fitted.line} is inside the safe area")
        // A cue with no line (SubRip) keeps the foot the view's padding gave it; SSA's foot line comes up to it.
        assertEquals(0.92f, plain.fit(Cue.Builder().setText("a").build()).line, 1e-6f)
        val ssa =
            twoLines
                .buildUpon()
                .setLine(0.95f, Cue.LINE_TYPE_FRACTION)
                .setLineAnchor(Cue.ANCHOR_TYPE_END)
                .build()
        assertEquals(0.92f, plain.fit(ssa).line, 1e-6f)
    }

    @Test
    fun `an SSA line at the foot of its screen is lifted like the rest, a sign at the top stays`() {
        val dialogue =
            Cue
                .Builder()
                .setText("a")
                .setLine(0.95f, Cue.LINE_TYPE_FRACTION)
                .setLineAnchor(Cue.ANCHOR_TYPE_END)
                .setPosition(0.5f)
                .setPositionAnchor(Cue.ANCHOR_TYPE_MIDDLE)
                .build()
        assertEquals(0.5f, lifted.fit(dialogue).line, 1e-6f)
        val sign =
            dialogue
                .buildUpon()
                .setLine(
                    0.05f,
                    Cue.LINE_TYPE_FRACTION,
                ).setLineAnchor(Cue.ANCHOR_TYPE_START)
                .build()
        assertEquals(0.05f, lifted.fit(sign).line, 1e-6f)
    }

    @Test
    fun `lines kept beside a wide card are counted as they wrap there, and the size is set for them`() {
        val upNext = Rect(left = 520f, top = 150f, right = 884f, bottom = 320f)
        // Room for two lines (50 px) but not for four (100 px), nor for four at 85%.
        val frame =
            cueFrame(
                height = 400f,
                width = 900f,
                foot = foot,
                ceiling = 220f,
                text = text,
                corner = upNext,
                lines = { span -> if (span < 900f) 4 else 2 },
            )
        assertEquals(520f / 900f, frame.width, 1e-6f)
        assertEquals(text * 80f / 100f, frame.text, 1e-6f)
    }

    @Test
    fun `a cue's lines are its own, each wrapped at about half the text size a character`() {
        val long = Cue.Builder().setText("x".repeat(84) + "\nshort").build()
        assertEquals(2, cueLines(listOf(long), textPx = 20f, spanPx = 900f))
        assertEquals(3, cueLines(listOf(long), textPx = 20f, spanPx = 520f))
        val picture = Cue.Builder().setBitmap(Bitmap.createBitmap(4, 2, Bitmap.Config.ARGB_8888)).build()
        assertEquals(0, cueLines(listOf(picture), textPx = 20f, spanPx = 900f))
    }

    @Test
    fun `cues that would meet the transport draw at 85%, smaller to clear it, never under 75%`() {
        fun scale(room: Float) =
            cueFrame(height = 400f, width = 900f, foot = 300f, ceiling = 300f - room, text = 0.1f, lines = { 2 }).text /
                0.1f
        // Two lines at 0.1 of 400 px: a box of 100 px.
        assertEquals(1f, scale(100f), 1e-6f)
        assertEquals(CUE_SHRINK, scale(90f), 1e-6f)
        assertEquals(0.8f, scale(80f), 1e-6f)
        assertEquals(CUE_FLOOR, scale(10f), 1e-6f)
    }
}
