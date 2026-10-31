package app.marquee.player

import android.graphics.Color
import android.text.Layout
import android.text.Spanned
import android.text.style.BackgroundColorSpan
import android.text.style.RelativeSizeSpan
import androidx.media3.common.text.Cue
import androidx.media3.ui.CaptionStyleCompat
import app.marquee.core.model.SubtitleLayout
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The second subtitle line's rules (plans/dual-subtitles.md 4, 6.2): the join, the layouts, the gates. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class SecondLineTest {
    private fun text(words: String) = Cue.Builder().setText(words).build()

    private fun bottomLine(
        words: String,
        n: Int,
    ) = Cue
        .Builder()
        .setText(words)
        .setLine(-1f - n, Cue.LINE_TYPE_NUMBER)
        .build()

    /** A sign its file placed at the top, as an ASS `\an8` line comes. */
    private val sign =
        Cue
            .Builder()
            .setText("PARIS, 1942")
            .setLine(0.1f, Cue.LINE_TYPE_FRACTION)
            .setLineAnchor(Cue.ANCHOR_TYPE_START)
            .build()

    /** The second line's cues, one of them placed by its file: its placement is dropped. */
    private val second =
        listOf(
            text("Hello"),
            Cue
                .Builder()
                .setText("there")
                .setLine(0.1f, Cue.LINE_TYPE_FRACTION)
                .build(),
        )

    @Test
    fun `Stacked joins the second line at 85 percent over the first line's foot cues, highest first, in one cue`() {
        val first = listOf(bottomLine("foot", 0), bottomLine("over", 1), sign)
        val drawn = twoLines(first, second, SubtitleLayout.Stacked, top = 0f, box = Color.BLACK)
        assertEquals(2, drawn.size, "the foot cues were not joined into one")
        assertEquals(sign, drawn[0], "the first line's sign moved")
        val joined = drawn[1]
        assertEquals("Hello\nthere\nover\nfoot", joined.text.toString())
        // One cue from the foot: CueFrame.fit hangs it as the single block.
        assertEquals(Cue.DIMEN_UNSET, joined.line)
        val spans = (joined.text as Spanned).getSpans(0, joined.text!!.length, RelativeSizeSpan::class.java)
        assertEquals(1, spans.size)
        assertEquals(SECOND_SIZE, spans[0].sizeChange)
        assertEquals(0.85f, SECOND_SIZE)
        assertEquals("Hello\nthere".length, (joined.text as Spanned).getSpanEnd(spans[0]))
        assertFalse(joined.windowColorSet, "Stacked keeps a box per line")
        // The joined cue hangs from the frame's foot as one.
        val fitted = CueFrame(bottom = 0.1f, text = 0.05f).fit(joined)
        assertEquals(0.9f, fitted.line, 1e-6f)
        assertEquals(Cue.ANCHOR_TYPE_END, fitted.lineAnchor)
    }

    @Test
    fun `the joined block keeps the first line's place across, a left or right aligned file's, never the second's`() {
        // A SubRip {\an1} line as Media3 hands it over: at the foot, its block from 8% of the width at the left.
        val left =
            Cue
                .Builder()
                .setText("foot")
                .setLine(0.92f, Cue.LINE_TYPE_FRACTION)
                .setLineAnchor(Cue.ANCHOR_TYPE_END)
                .setPosition(0.08f)
                .setPositionAnchor(Cue.ANCHOR_TYPE_START)
                .build()
        // A WebVTT `align:right` line: its words to the right edge of a block that ends at the right.
        val right =
            bottomLine("foot", 0)
                .buildUpon()
                .setTextAlignment(Layout.Alignment.ALIGN_OPPOSITE)
                .setPosition(1f)
                .setPositionAnchor(Cue.ANCHOR_TYPE_END)
                .setSize(0.6f)
                .build()
        // The second line's own placement (centred, from the left at 30%) is dropped.
        val placedSecond =
            Cue
                .Builder()
                .setText("Hello")
                .setTextAlignment(Layout.Alignment.ALIGN_CENTER)
                .setPosition(0.3f)
                .setPositionAnchor(Cue.ANCHOR_TYPE_START)
                .build()
        for (layout in listOf(SubtitleLayout.Stacked, SubtitleLayout.OneBox)) {
            val l = twoLines(listOf(left), listOf(placedSecond), layout, 0f, Color.BLACK).single()
            assertNull(l.textAlignment)
            assertEquals(0.08f, l.position)
            assertEquals(Cue.ANCHOR_TYPE_START, l.positionAnchor)
            assertEquals(Cue.DIMEN_UNSET, l.size)
            // A higher foot cue of its own placement does not move the block: the lowest one places it.
            val r =
                twoLines(
                    listOf(left.buildUpon().setLine(-2f, Cue.LINE_TYPE_NUMBER).build(), right),
                    listOf(placedSecond),
                    layout,
                    0f,
                    Color.BLACK,
                ).single()
            assertEquals(Layout.Alignment.ALIGN_OPPOSITE, r.textAlignment)
            assertEquals(1f, r.position)
            assertEquals(Cue.ANCHOR_TYPE_END, r.positionAnchor)
            assertEquals(0.6f, r.size)
            // The frame hangs it from the foot and leaves its place across alone.
            val fitted = CueFrame(bottom = 0.1f, text = 0.05f).fit(r)
            assertEquals(Layout.Alignment.ALIGN_OPPOSITE, fitted.textAlignment)
            assertEquals(1f, fitted.position)
            // Beside a corner control the frame's narrowing wins: the block centres in the room left of it.
            val narrowed = CueFrame(bottom = 0.1f, text = 0.05f, width = 0.6f, center = 0.3f).fit(r)
            assertEquals(0.3f, narrowed.position)
            assertEquals(Cue.ANCHOR_TYPE_MIDDLE, narrowed.positionAnchor)
            assertEquals(0.6f, narrowed.size)
        }
        // A centred first line leaves the block centred: nothing set, as before.
        val centred =
            twoLines(
                listOf(bottomLine("foot", 0)),
                listOf(placedSecond),
                SubtitleLayout.Stacked,
                0f,
                Color.BLACK,
            ).single()
        assertNull(centred.textAlignment)
        assertEquals(Cue.DIMEN_UNSET, centred.position)
        assertEquals(Cue.TYPE_UNSET, centred.positionAnchor)
    }

    @Test
    fun `with no second cue the first line draws as it is, and with no first cue the second sits at the foot`() {
        val first = listOf(bottomLine("foot", 0))
        assertEquals(first, twoLines(first, emptyList(), SubtitleLayout.Stacked, 0f, Color.BLACK))
        val alone = twoLines(emptyList(), listOf(text("Hello")), SubtitleLayout.Stacked, 0f, Color.BLACK)
        assertEquals(listOf("Hello"), alone.map { it.text.toString() })
    }

    @Test
    fun `Split keeps the first line and hangs the second, full size, from the top`() {
        val first = listOf(bottomLine("foot", 0))
        val drawn = twoLines(first, second, SubtitleLayout.Split, top = 0.12f, box = Color.BLACK)
        assertEquals(first[0], drawn[0])
        val upper = drawn[1]
        assertEquals("Hello\nthere", upper.text.toString())
        assertEquals(0.12f, upper.line)
        assertEquals(Cue.LINE_TYPE_FRACTION, upper.lineType)
        assertEquals(Cue.ANCHOR_TYPE_START, upper.lineAnchor)
        assertEquals(0, (upper.text as Spanned).getSpans(0, upper.text!!.length, RelativeSizeSpan::class.java).size)
        // The frame leaves it where it is.
        val frame = CueFrame(bottom = 0.1f, text = 0.05f)
        assertEquals(upper, frame.fit(upper))
    }

    @Test
    fun `Split's top mirrors the foot, under the 5 percent line, the top bar and a sign in the top half`() {
        val h = 540f
        val w = 960f
        // A 2.39:1 picture on a 16:9 TV: 401.7 px tall from 69.2 px down; 8% of it under its top.
        val picture = w / 2.39f
        val mirror = (h - picture) / 2 + picture * CUE_RISE
        assertEquals(mirror, secondTop(h, w, 2.39f, bar = 0f, first = emptyList(), textPx = 27f, gap = 8f), 1e-3f)
        // Full frame: 8% of the screen, over the 5% line.
        assertEquals(h * 0.08f, secondTop(h, w, 16f / 9f, 0f, emptyList(), 27f, 8f), 1e-3f)
        // A picture as tall as the screen never reaches over the 5% line: a 4:3 picture's 8% is 43.2 px, the line 27.
        assertTrue(secondTop(h, w, 4f / 3f, 0f, emptyList(), 27f, 8f) >= h * 0.05f)
        // The controls up: under the top bar's foot.
        assertEquals(150f, secondTop(h, w, 16f / 9f, bar = 150f, first = emptyList(), textPx = 27f, gap = 8f))
        // A sign at 10% from the top, one line of 27 px: its foot at 54 + 33.75, then 8 clear.
        assertEquals(54f + 1.25f * 27f + 8f, secondTop(h, w, 16f / 9f, 0f, listOf(sign), 27f, 8f), 1e-3f)
        // A sign in the lower half, or a line from the foot, is no sign to dodge.
        val low =
            Cue
                .Builder()
                .setText("low")
                .setLine(0.7f, Cue.LINE_TYPE_FRACTION)
                .build()
        assertEquals(h * 0.08f, secondTop(h, w, 16f / 9f, 0f, listOf(low, bottomLine("foot", 0)), 27f, 8f), 1e-3f)
    }

    @Test
    fun `One box draws both lines in the caption window, the lines' own background cleared`() {
        val drawn =
            twoLines(
                listOf(bottomLine("foot", 0)),
                listOf(text("Hello")),
                SubtitleLayout.OneBox,
                0f,
                0x99000000.toInt(),
            )
        val joined = drawn.single()
        assertTrue(joined.windowColorSet)
        assertEquals(0x99000000.toInt(), joined.windowColor)
        val clear = (joined.text as Spanned).getSpans(0, joined.text!!.length, BackgroundColorSpan::class.java)
        assertEquals(listOf(Color.TRANSPARENT), clear.map { it.backgroundColor })
        assertEquals(joined.text!!.length, (joined.text as Spanned).getSpanEnd(clear[0]))
    }

    @Test
    fun `One box takes the user's caption window when their style sets one, else the caption background`() {
        val plain = CaptionStyleCompat.DEFAULT
        assertEquals(plain.backgroundColor, boxColor(plain))
        val user =
            CaptionStyleCompat(
                Color.YELLOW,
                0x80000000.toInt(),
                0xCC102030.toInt(),
                CaptionStyleCompat.EDGE_TYPE_OUTLINE,
                Color.BLACK,
                null,
            )
        assertEquals(0xCC102030.toInt(), boxColor(user))
    }

    @Test
    fun `Only when paused shows the second line while the viewer has paused, a stall included, else always`() {
        assertTrue(secondShows(onPause = false, playWhenReady = true))
        assertTrue(secondShows(onPause = false, playWhenReady = false))
        assertFalse(secondShows(onPause = true, playWhenReady = true))
        assertTrue(secondShows(onPause = true, playWhenReady = false))
    }

    @Test
    fun `the second line follows the core's as a level until the viewer picks one`() {
        assertEquals("en", followSecond(auto = "en", chose = false, current = null))
        assertNull(followSecond(auto = null, chose = false, current = "en"), "the core's null did not turn it off")
        assertEquals("de", followSecond(auto = "de", chose = false, current = "en"))
        assertEquals("de", followSecond(auto = "en", chose = true, current = "de"))
        assertNull(followSecond(auto = "en", chose = true, current = null), "the viewer's Off did not hold")
    }

    @Test
    fun `two lines are one language by their base language, an unknown one never`() {
        assertTrue(samePrimaryLanguage("eng", "en-GB"))
        assertTrue(samePrimaryLanguage("pt", "pt-BR"))
        assertTrue(samePrimaryLanguage("tur", "tr"))
        assertFalse(samePrimaryLanguage("en", "tr"))
        assertFalse(samePrimaryLanguage("und", "und"))
        assertFalse(samePrimaryLanguage(null, "en"))
        assertFalse(samePrimaryLanguage("", ""))
    }

    @Test
    fun `the Layout row steps Stacked, Split, One box and stops at the ends`() {
        assertEquals(SubtitleLayout.Split, stepLayout(SubtitleLayout.Stacked, 1))
        assertEquals(SubtitleLayout.OneBox, stepLayout(SubtitleLayout.Split, 1))
        assertNull(stepLayout(SubtitleLayout.OneBox, 1))
        assertNull(stepLayout(SubtitleLayout.Stacked, -1))
        assertEquals(SubtitleLayout.Split, stepLayout(SubtitleLayout.Unknown, 1))
    }

    @Test
    fun `the Subtitles button says both languages while two lines show`() {
        val first =
            listOf(
                TrackChoice("off", "Off"),
                TrackChoice("late:tr", "Turkish 1", selected = true, section = "Turkish", language = "tr"),
            )
        val second =
            listOf(
                TrackChoice("off", "Off"),
                TrackChoice("late:en", "English 1", selected = true, section = "English", language = "en"),
            )
        assertEquals("Turkish + English", subtitlesDetail(first, false, second))
        assertEquals("Turkish", subtitlesDetail(first, false, null))
    }
}
