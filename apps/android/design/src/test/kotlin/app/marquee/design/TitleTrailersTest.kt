package app.marquee.design

import android.app.ActivityManager
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A TV form plays the title header's trailer only above its measured memory floor (plans/trailer-inline.md 4). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TitleTrailersTest {
    private val context = RuntimeEnvironment.getApplication()
    private val am = context.getSystemService(ActivityManager::class.java)

    private fun device(
        totalMem: Long,
        lowRam: Boolean = false,
        pressed: Boolean = false,
    ) {
        shadowOf(am).setMemoryInfo(
            ActivityManager.MemoryInfo().apply {
                this.totalMem = totalMem
                availMem = totalMem / 2
                lowMemory = pressed
            },
        )
        shadowOf(am).setIsLowRamDevice(lowRam)
    }

    @Test
    fun `the floor, the device's flag, the pressure and the free room each decide`() {
        val gb = 1_000_000_000L
        // Sizes as totalMem reads them (a little under the label): 1.5, 2, 3, 4 and 6 GB devices.
        val sizes = listOf(1_400_000_000L, 1_850_000_000L, 2_800_000_000L, 3_700_000_000L, 5_500_000_000L)
        val passes =
            sizes.map {
                trailerCapable(
                    it,
                    gb,
                    lowRam = false,
                    lowMemory = false,
                    floor = TitleTrailers.TV_FLOOR,
                )
            }
        assertEquals(sizes.map { it >= TitleTrailers.TV_FLOOR }, passes)
        val big = 12 * gb
        assertTrue(trailerCapable(big, gb, lowRam = false, lowMemory = false, floor = TitleTrailers.TV_FLOOR))
        assertFalse(trailerCapable(big, gb, lowRam = true, lowMemory = false, floor = TitleTrailers.TV_FLOOR))
        assertFalse(trailerCapable(big, gb, lowRam = false, lowMemory = true, floor = TitleTrailers.TV_FLOOR))
        assertFalse(
            trailerCapable(
                big,
                TitleTrailers.ROOM - 1,
                lowRam = false,
                lowMemory = false,
                floor = TitleTrailers.TV_FLOOR,
            ),
        )
        assertFalse(
            trailerCapable(
                TitleTrailers.TV_FLOOR - 1,
                gb,
                lowRam = false,
                lowMemory = false,
                floor = TitleTrailers.TV_FLOOR,
            ),
        )
    }

    @Test
    fun `a TV at its floor plays the header, one under it and a low-RAM one do not`() {
        device(TitleTrailers.TV_FLOOR)
        assertTrue(TrailerPreviews.headerOnTv(context))
        device(TitleTrailers.TV_FLOOR - 1)
        assertFalse(TrailerPreviews.headerOnTv(context))
        // A Fire TV stick: 2 GB, and the system calls it low on RAM.
        device(2_000_000_000L, lowRam = true)
        assertFalse(TrailerPreviews.headerOnTv(context))
        device(TitleTrailers.TV_FLOOR, lowRam = true)
        assertFalse(TrailerPreviews.headerOnTv(context))
    }

    private fun capable(floor: Long) =
        TrailerPreviews.signals(context, true, false, tv = false, hasTrailer = true, minMemory = floor).capable

    @Test
    fun `under memory pressure right now no header plays`() {
        device(TitleTrailers.TV_FLOOR, pressed = true)
        assertFalse(capable(TitleTrailers.TV_FLOOR))
        device(TitleTrailers.TV_FLOOR)
        assertTrue(capable(TitleTrailers.TV_FLOOR))
    }

    @Test
    fun `the header's signals use the floor they are given`() {
        device(TitleTrailers.TV_FLOOR)
        assertTrue(
            TrailerPreviews
                .signals(
                    context,
                    true,
                    false,
                    tv = false,
                    hasTrailer = true,
                    minMemory = TitleTrailers.TV_FLOOR,
                ).capable,
        )
        assertFalse(
            TrailerPreviews
                .signals(
                    context,
                    true,
                    false,
                    tv = false,
                    hasTrailer = true,
                    minMemory =
                        TitleTrailers.TV_FLOOR + 1,
                ).capable,
        )
    }
}
