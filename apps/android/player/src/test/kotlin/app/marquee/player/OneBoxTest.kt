package app.marquee.player

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.view.View
import androidx.media3.common.text.Cue
import androidx.media3.ui.CaptionStyleCompat
import androidx.media3.ui.SubtitleView
import app.marquee.core.model.SubtitleLayout
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * One box drawn by Media3's own SubtitleView (plans/dual-subtitles.md 4 E): one box fitted to both lines and centred
 * on them, the Pixel walk's failure (2026-11-01) being a band from 23% of the width to the right edge.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class OneBoxTest {
    /** The Pixel 9 Pro's screen in landscape. */
    private val width = 2856
    private val height = 1280

    /** A WebVTT foot line as Media3 hands it over: centred, 100% wide. */
    private fun vtt(words: String) =
        Cue
            .Builder()
            .setText(words)
            .setLine(-1f, Cue.LINE_TYPE_NUMBER)
            .setPosition(0.5f)
            .setPositionAnchor(Cue.ANCHOR_TYPE_MIDDLE)
            .setSize(1f)
            .build()

    private val first = listOf(vtt("...savunmasını öğretebiliriz."))
    private val second = listOf(vtt("...from even the most skilled extractor."))

    /** The columns the box takes and the columns the text takes, px. */
    private data class Drawn(
        val box: IntRange,
        val text: IntRange,
    )

    private fun draw(frame: CueFrame): Drawn {
        val style = CaptionStyleCompat.DEFAULT
        val cues = twoLines(first, second, SubtitleLayout.OneBox, top = 0f, box = boxColor(style))
        val view = SubtitleView(RuntimeEnvironment.getApplication())
        view.setStyle(style)
        view.setFractionalTextSize(frame.text)
        view.setCues(fitted(cues, frame, style, width.toFloat(), height.toFloat()))
        view.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        view.layout(0, 0, width, height)
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(bitmap))
        val pixels = IntArray(width * height).also { bitmap.getPixels(it, 0, width, 0, 0, width, height) }

        fun columns(hit: (Int) -> Boolean): IntRange {
            val xs = (0 until width).filter { x -> (0 until height).any { y -> hit(pixels[y * width + x]) } }
            assertTrue(xs.isNotEmpty(), "nothing drawn")
            return xs.first()..xs.last()
        }
        return Drawn(
            box = columns { Color.alpha(it) > 0 },
            text = columns { Color.red(it) > 128 },
        )
    }

    private fun assertFitted(
        drawn: Drawn,
        center: Float,
    ) {
        val (box, text) = drawn
        val boxWidth = box.last - box.first
        val textWidth = text.last - text.first
        // The box holds the text with a padding, never a band: under a fifth of the text's width more.
        assertTrue(box.first < text.first && box.last > text.last, "$drawn")
        assertTrue(boxWidth < textWidth * 1.2f, "a band, not a box around the lines: $drawn")
        // Centred on the text, and where the block belongs across.
        val mid = { r: IntRange -> (r.first + r.last) / 2f }
        assertTrue(abs(mid(box) - mid(text)) <= 2f, "off centre: $drawn")
        assertEquals(center * width, mid(box), width * 0.01f, "$drawn")
    }

    @Test
    fun `One box fits both lines and sits centred`() {
        assertFitted(draw(CueFrame(bottom = 0.1f, text = SubtitleView.DEFAULT_TEXT_SIZE_FRACTION)), center = 0.5f)
    }

    @Test
    fun `beside a corner control One box fits both lines in the room left of it`() {
        val frame =
            CueFrame(bottom = 0.2f, text = SubtitleView.DEFAULT_TEXT_SIZE_FRACTION, width = 0.54f, center = 0.4f)
        assertFitted(draw(frame), center = 0.4f)
    }

    @Test
    fun `a line wider than the room keeps the room's width and wraps`() {
        val long =
            Cue
                .Builder()
                .setText("x".repeat(400))
                .setWindowColor(Color.BLACK)
                .build()
        val paint = android.text.TextPaint().apply { textSize = 40f }
        assertEquals(Cue.DIMEN_UNSET, long.hugged(paint, width.toFloat()).size)
        // A cue with no window of its own (Stacked, a plain line) is left as it is.
        val plain = vtt("short")
        assertEquals(plain, plain.hugged(paint, width.toFloat()))
    }
}
