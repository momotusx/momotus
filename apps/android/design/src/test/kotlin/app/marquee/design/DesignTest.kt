package app.marquee.design

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.unit.dp
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
        // A chip's detail (Discover's catalog source), as Chip draws it: `dim` on the segment track over
        // the page and over `surface`, and the label's own white on the selected grey; 4.5:1 either way.
        val rest = chipDetailColor(selected = false, colors = dark)
        val picked = chipDetailColor(selected = true, colors = dark)
        assertTrue(contrast(rest, SEGMENT_TRACK) >= 4.5f) // 6.27
        assertTrue(contrast(rest, SEGMENT_TRACK, dark.surface) >= 4.5f) // 5.46
        assertTrue(contrast(picked, Tokens.selectedFill) >= 4.5f) // 5.37
        // One white: selection is a grey that never reads as the white focus.
        assertTrue(contrast(dark.primary, Tokens.selectedFill) >= 3f)
        // The control edge still drawn on the off switch until phase 8: 3:1 on the page.
        assertTrue(contrast(dark.control, dark.bg) >= 3f) // 3.25
        // Dark only: the light palette mirrors the dark one.
        assertEquals(dark, Tokens.light)
    }

    @Test
    fun `the segmented control and the secondary pill carry their state in the label`() {
        val dark = Tokens.dark
        // Apple's segmented control (design/reference/apple-tv.md): the selected segment is
        // `selected.fill` with a white label, the others a 64 percent label on the track. Selection is
        // carried by the label's level and the fill, so the fill against the track is not an edge that
        // needs 3:1; the labels are text and need 4.5:1.
        assertEquals(Color(118, 118, 128).copy(alpha = 0.24f).toArgb(), SEGMENT_TRACK.toArgb())
        assertTrue(contrast(Color.White, Tokens.selectedFill) >= 4.5f) // 5.99
        assertTrue(contrast(dark.dim, SEGMENT_TRACK) >= 4.5f) // 6.27
        // A secondary is white on a 16 percent white fill, never a colored word: 10.30 on the page.
        // Over artwork the scrims keep it at 4.5:1 (the backdrop check below).
        assertEquals(Color.White.copy(alpha = 0.16f).toArgb(), SECONDARY_FILL.toArgb())
        assertTrue(contrast(Color.White, SECONDARY_FILL) >= 4.5f)
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

    @Test
    fun `the primary stays the white pill when focused, and a TV focus whitens a secondary`() {
        val dark = Tokens.dark
        // The Skip pill and every primary: white, focused or not, with no ring around it on TV.
        assertEquals(dark.primary, buttonFill(primary = true, selected = false, tvFocused = false, colors = dark))
        assertEquals(
            dark.primary,
            buttonFill(primary = true, selected = false, tvFocused = true, colors = dark.inverted()),
        )
        // A secondary is the 16 percent fill; focused on TV (its palette inverted) it is the white fill, a
        // selected one too.
        assertEquals(SECONDARY_FILL, buttonFill(primary = false, selected = false, tvFocused = false, colors = dark))
        assertEquals(
            dark.primary,
            buttonFill(primary = false, selected = false, tvFocused = true, colors = dark.inverted()),
        )
        assertEquals(
            dark.primary,
            buttonFill(primary = false, selected = true, tvFocused = true, colors = dark.inverted()),
        )
        // While another button holds the TV focus the primary yields its white for the secondary fill:
        // one white per screen.
        assertEquals(
            SECONDARY_FILL,
            buttonFill(primary = true, selected = false, tvFocused = false, colors = dark, yields = true),
        )
        // Inside the player (its ring kept) a secondary and a round button keep the opaque raised fill: their
        // controls sit over the picture with no scrim until phase 5 moves both.
        assertEquals(
            dark.raised,
            buttonFill(primary = false, selected = false, tvFocused = false, colors = dark, ringed = true),
        )
        // A selected toggle is the selected grey (the selected.fill token, #636366), never white.
        assertEquals(Color(0xFF636366), Tokens.selectedFill)
        assertEquals(
            Tokens.selectedFill,
            buttonFill(primary = false, selected = true, tvFocused = false, colors = dark),
        )
    }

    @Test
    fun `TV focus lifts buttons and rows by the rise token and art by the deeper art tokens`() {
        // design/tokens.json focus and lift (the frames and the web): buttons 1.06, rows 1.03, art 1.1.
        assertEquals(1.06f, Tokens.FOCUS_SCALE_CARD)
        assertEquals(1.03f, Tokens.FOCUS_SCALE_TILE)
        assertEquals(1.1f, Tokens.FOCUS_SCALE_ART)
        assertEquals(8.dp, Tokens.focusRise)
        assertEquals(10.dp, Tokens.focusRiseArt)
        // The art shadow, 0 44px 88px at .55.
        assertEquals(44.dp, Tokens.focusArtShadowY)
        assertEquals(88.dp, Tokens.focusArtShadowBlur)
        assertEquals(0.55f, Tokens.FOCUS_ART_SHADOW_ALPHA)
    }

    @Test
    fun `the TV strokes no ring, the phone and the TV player stroke the 4 dp one`() {
        assertEquals(0.dp, ringWidth(tv = true, ringed = false))
        assertEquals(Tokens.focusRing, ringWidth(tv = false, ringed = false))
        assertEquals(Tokens.focusRing, ringWidth(tv = true, ringed = true))
    }

    @Test
    fun `a TV focus turns a row white, and its words, glyphs and states stay readable`() {
        val dark = Tokens.dark
        val on = dark.inverted()
        // The fill is the white primary; nothing of the row's own edge is left on it.
        assertEquals(dark.primary, on.surface)
        assertEquals(dark.primary, on.raised)
        assertEquals(0f, on.edge.alpha)
        // Every word and state the palette gives reads at 4.5:1 on the white (white-in-v3 states go dark).
        val words =
            mapOf(
                "text" to on.text,
                "muted" to on.muted,
                "dim" to on.dim,
                "accent" to on.accent,
                "ok" to on.ok,
                "warn" to on.warn,
                "bad" to on.bad,
                "star" to on.star,
            )
        for ((name, ink) in words) assertTrue(contrast(ink, on.surface) >= 4.5f, "$name on the focus white")
    }

    @Test
    fun `over white art every word on a backdrop keeps 4_5 to 1, a secondary on its fill and text on the scrim`() {
        // Each at its top trailing corner, as a share of its backdrop: [x] from the leading edge, [y] from the
        // bottom. Laid in order: white art, the side scrim, the foot scrim, then the 16 percent fill under a
        // secondary's white label; bare text in its own level.
        class Spot(
            val name: String,
            val side: List<ScrimStop>,
            val x: Float,
            val foot: List<ScrimStop>,
            val y: Float,
            val page: Color,
            val ink: Color = Color.White,
            val fill: Color = SECONDARY_FILL,
        )
        val tv = formPalette(Form.Tv)
        val phone = formPalette(Form.Phone)
        val bare = Color.Transparent
        // TvHome's 1760 by 560 hero card: the text column 640 wide from 56 in ends at x .4; from its foot the
        // buttons sit at 48, the reason line ends by .38, the synopsis by .6 and the meta line by .69.
        val (side, foot) = Scrims.heroSide to Scrims.heroFoot
        val spots =
            listOf(
                Spot("TV Home, Details (its glyph ends by 321)", side, 0.2f, foot, 0.27f, tv.bg),
                Spot("TV Home, the reason line", side, 0.4f, foot, 0.38f, tv.bg, tv.text, bare),
                Spot("TV Home, the synopsis", side, 0.4f, foot, 0.6f, tv.bg, tv.muted, bare),
                Spot("TV Home, the meta line", side, 0.4f, foot, 0.69f, tv.bg, tv.muted, bare),
                // The TV title's Play and round buttons end before the middle, the row anywhere down the column;
                // its synopsis, 640 wide from 80 in, ends by x .4.
                Spot("TV title, its last round button", Scrims.titleSide, 0.5f, Scrims.titleFoot, 0.5f, tv.bg),
                Spot("TV title, the synopsis", Scrims.titleSide, 0.4f, Scrims.titleFoot, 0.7f, tv.bg, tv.muted, bare),
                // PhoneHome's hero card: Play and Details 24 from its foot, a fifth of it up (a third in landscape);
                // its text starts no higher than .7.
                Spot("phone Home, Play and Details", emptyList(), 0f, Scrims.phoneHeroFoot, 0.35f, phone.bg),
                Spot("phone Home, its text", emptyList(), 0f, Scrims.phoneHeroFoot, 0.7f, phone.bg, phone.muted, bare),
            )
        for (spot in spots) {
            val covered = 1 - (1 - Scrims.alpha(spot.side, spot.x)) * (1 - Scrims.alpha(spot.foot, spot.y))
            val art = spot.page.copy(alpha = covered).compositeOver(Color.White)
            val ratio = contrast(spot.ink, spot.fill, art)
            assertTrue(ratio >= 4.5f, "${spot.name}: $ratio")
        }
    }

    @Test
    fun `the phone page is black with the iPhone's greys, and every word on it reads`() {
        val phone = formPalette(Form.Phone)
        val dark = Tokens.dark
        assertEquals(Color.Black, phone.bg)
        assertEquals(dark.surfacePhone, phone.surface)
        assertEquals(dark.raisedPhone, phone.raised)
        assertEquals(dark.dimPhone, phone.dim)
        assertEquals(dark.primaryPhone, phone.primary)
        assertEquals(dark, formPalette(Form.Tv))
        for (ground in listOf(phone.bg, phone.surface, phone.raised)) {
            for ((name, ink) in mapOf("text" to phone.text, "muted" to phone.muted, "dim" to phone.dim)) {
                assertTrue(contrast(ink, ground, phone.bg) >= 4.5f, "$name on $ground")
            }
        }
        assertTrue(contrast(phone.onPrimary, phone.primary) >= 4.5f)
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
