package app.marquee.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class FormatTest {
    @Test
    fun `sizes read in the 1024-based units addons print`() {
        val gib = 1L shl 30
        assertEquals("5 GB", Format.bytes(5 * gib))
        assertEquals("7.4 GB", Format.bytes((7.4 * gib).toLong()))
        assertEquals("1.2 GB", Format.bytes(1_267_015_352))
        assertEquals("120 GB", Format.bytes(120 * gib + gib / 3))
        assertEquals("14 MB", Format.bytes(14_621_544))
        assertEquals("1 KB", Format.bytes(10))
    }

    @Test
    fun clock() {
        assertEquals("2:41", Format.clock(161))
        assertEquals("1:02:05", Format.clock(3725))
        assertEquals("0:00", Format.clock(-4))
    }

    @Test
    fun `series video ids carry season and episode, movies do not`() {
        assertEquals(Format.VideoRef("tt0944947", 2, 1), Format.parseVideo("tt0944947:2:1"))
        assertNull(Format.parseVideo("tt1727587"))
        // A digit run that does not fit an Int is not a season or an episode, and must not throw.
        assertNull(Format.parseVideo("x:99999999999999999999:1"))
        assertEquals("S2:E1", Format.episodeLabel(2, 1))
        assertEquals("Specials", Format.seasonName(0))
    }

    @Test
    fun `runtimes like Cinemeta writes them`() {
        assertEquals(57, Format.runtimeMinutes("57 min"))
        assertEquals(112, Format.runtimeMinutes("1h 52min"))
        assertNull(Format.runtimeMinutes(null))
    }

    @Test
    fun `release names break after dots but keep numbers whole`() {
        assertEquals(
            listOf("Show.", "S02E03.2160p.", "WEB-DL.", "DDP5.1.", "H.265", "-FLUX"),
            Format.releaseParts("Show.S02E03.2160p.WEB-DL.DDP5.1.H.265-FLUX"),
        )
    }
}
