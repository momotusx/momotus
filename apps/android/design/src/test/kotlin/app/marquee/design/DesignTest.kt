package app.marquee.design

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import kotlin.test.Test
import kotlin.test.assertEquals

class DesignTest {
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
        // design/DECISION.md v2: text 4.5:1 floors (accent 4.66:1 on bg), control edges 3:1.
        val dark = Tokens.dark
        assertEquals(true, contrast(dark.text, dark.bg) > 15f)
        assertEquals(true, contrast(dark.accent, dark.bg) >= 4.5f)
        assertEquals(true, contrast(dark.muted, dark.surface) >= 4.5f)
        assertEquals(true, contrast(dark.dim, dark.bg) >= 4.5f)
        assertEquals(true, contrast(dark.onPrimary, dark.primary) >= 4.5f)
        assertEquals(true, contrast(dark.control, dark.bg) >= 3f)
        // Dark only: the light palette mirrors the dark one.
        assertEquals(dark, Tokens.light)
    }

    @Test
    fun `glass controls stay visible on any artwork`() {
        // plans/android-design-v2.md 1.2: over art, the boundary is the fill against the art or the
        // control edge against the fill, whichever is larger: 3:1 either way; text on it 4.5:1.
        val dark = Tokens.dark
        for (art in listOf(Color.Black, Color(0xFF808080), Color.White)) {
            val fill = dark.glassStrong.compositeOver(art)
            val boundary = maxOf(contrast(fill, art), contrast(dark.control, fill))
            assertEquals(true, boundary >= 3f, "boundary $boundary on $art")
            assertEquals(true, contrast(dark.text, fill) >= 4.5f, "text on $art")
        }
        // The focus ring against the page.
        assertEquals(true, contrast(dark.accent, dark.bg) >= 3f)
    }

    private fun contrast(
        a: Color,
        b: Color,
    ): Float {
        val la = a.luminance() + 0.05f
        val lb = b.luminance() + 0.05f
        return maxOf(la, lb) / minOf(la, lb)
    }
}
