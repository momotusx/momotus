package app.marquee.design

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class DesignTest {
    @Test
    fun `a switch is a white track with a dark knob when on, an empty track with the control edge when off`() {
        for (palette in listOf(Tokens.dark, Tokens.light)) {
            val on = switchColors(true, palette)
            assertEquals(SwitchColors(palette.primary, Color.Transparent, palette.onPrimary), on)
            val off = switchColors(false, palette)
            assertEquals(SwitchColors(Color.Transparent, palette.control, palette.muted), off)
            // Never the accent.
            assertTrue(listOf(on, off).none { palette.accent in listOf(it.track, it.edge, it.knob) })
        }
    }

    @Test
    fun `a 1080p buffer is one pixel per point and 720p and 4K scale with it`() {
        assertEquals(1f, tvScale(1920, 1080))
        assertEquals(2f, tvScale(3840, 2160))
        assertEquals(2f / 3f, tvScale(1280, 720), 0.0001f)
    }

    @Test
    fun `a window that is not 16 to 9 follows the smaller axis so nothing overflows`() {
        assertEquals(1f, tvScale(2560, 1080)) // ultra wide: the height decides
        assertEquals(0.5f, tvScale(960, 1080)) // narrow: the width decides
    }

    @Test
    fun `a zero sized first frame never gives a zero density`() {
        assertEquals(0.1f, tvScale(0, 0))
    }

    @Test
    fun `http posters are upgraded except on loopback`() {
        assertEquals("https://img.example/p.jpg", HttpsUpgrade.upgrade("http://img.example/p.jpg"))
        assertEquals("https://img.example/p.jpg", HttpsUpgrade.upgrade("https://img.example/p.jpg"))
        assertEquals("http://127.0.0.1:7780/p.jpg", HttpsUpgrade.upgrade("http://127.0.0.1:7780/p.jpg"))
    }

    @Test
    fun `the cache key carries the pixel size so a prefetch can match a tile`() {
        assertEquals("https://x/p.jpg@336x504", imageKey("https://x/p.jpg", 336, 504))
    }

    @Test
    fun `every color token has the contrast the design decision names`() {
        // design/DECISION.md v3: text 4.5:1, control edges, glyphs and the keyboard ring 3:1.
        val dark = Tokens.dark
        assertTrue(contrast(dark.text, dark.bg) >= 14f) // 14.51
        assertTrue(contrast(dark.muted, dark.surface) >= 4.5f)
        assertTrue(contrast(dark.dim, dark.bg) >= 4.5f)
        assertTrue(contrast(dark.dim, dark.surface) >= 4.5f)
        assertTrue(contrast(dark.onPrimary, dark.primary) >= 4.5f)
        assertTrue(contrast(dark.onKey, dark.keyFill) >= 4.5f) // a sheet's primary, 4.70
        // The key blue as text on the page, and as the keyboard ring on every surface.
        assertTrue(contrast(dark.key, dark.bg) >= 4.5f) // 4.66
        assertTrue(contrast(dark.key, dark.surface) >= 3f) // 3.82
        assertTrue(contrast(dark.key, dark.raised) >= 3f) // 3.11
        // The TV ring, white on the page.
        assertTrue(contrast(dark.accent, dark.bg) >= 3f)
        assertTrue(contrast(dark.switchOn, dark.surface) >= 3f) // 6.89
        // A chip's detail (Discover's catalog source): dim on `raised` unselected, and onPrimary at
        // 70 percent on the white pill selected (Chip in Components.kt).
        assertTrue(contrast(dark.dim, dark.raised) >= 4.5f) // 5.73
        assertTrue(contrast(dark.onPrimary.copy(alpha = dark.onPrimary.alpha * 0.7f), dark.primary) >= 4.5f) // 5.80
        // Dark only: the light palette mirrors the dark one.
        assertEquals(dark, Tokens.light)
    }

    @Test
    fun `the segmented control and the secondary pill carry their state in the label`() {
        val dark = Tokens.dark
        // Apple's segmented control (design/reference/apple-tv.md): the selected segment is `control`
        // with a white label, the others a 64 percent label on the track. Selection is carried by
        // the label's level and the fill, so the fill against the track (2.14:1) is not an edge
        // that needs 3:1; the labels are text and need 4.5:1.
        val track = Color(118, 118, 128).copy(alpha = 0.24f)
        assertTrue(contrast(Color.White, dark.control) >= 4.5f) // 5.99
        assertTrue(contrast(dark.dim, track) >= 4.5f) // 6.27
        // A secondary is white on a 16 percent white fill, never a colored word: 10.30 on the page.
        // Over artwork the ramps keep it at 4.5:1; that check lands with the backdrop piece.
        assertTrue(contrast(Color.White, Color.White.copy(alpha = 0.16f)) >= 4.5f)
    }

    @Test
    fun `text on the strong glass reads on any artwork`() {
        // v3 has no control edges: a glass panel over art is found by its contents, so the text on it
        // must read on black, grey and white art alike.
        val dark = Tokens.dark
        for (art in listOf(Color.Black, Color(0xFF808080), Color.White)) {
            assertTrue(contrast(dark.text, dark.glassStrong, art) >= 4.5f, "text on $art")
        }
    }

    /** WCAG contrast of [fg] on [bg]: a translucent [bg] is first laid on [base], then [fg] on that. */
    private fun contrast(
        fg: Color,
        bg: Color,
        base: Color = Tokens.dark.bg,
    ): Float {
        val back = bg.compositeOver(base)
        val la = fg.compositeOver(back).luminance() + 0.05f
        val lb = back.luminance() + 0.05f
        return maxOf(la, lb) / minOf(la, lb)
    }
}
