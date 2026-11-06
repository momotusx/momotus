package app.marquee.design

import android.app.ActivityManager
import android.content.Context
import coil3.Extras
import coil3.request.allowRgb565
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** A device under 2 GB decodes opaque art as RGB_565 (plans/batch-13-perf.md, android-perf); a larger one does not. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ImageLoaderMemoryTest {
    private fun rgb565(totalMem: Long): Boolean? {
        val context = RuntimeEnvironment.getApplication()
        val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        shadowOf(am).setMemoryInfo(ActivityManager.MemoryInfo().apply { this.totalMem = totalMem })
        return createImageLoader(context).defaults.extras[Extras.Key.allowRgb565]
    }

    @Test
    fun `a stick under 2 GB decodes opaque art at half the bytes, a phone does not`() {
        // The Fire TV Stick 4K: 1,316,676 kB.
        assertEquals(true, rgb565(1_316_676L * 1024))
        assertEquals(false, rgb565(RGB565_BELOW))
        assertEquals(false, rgb565(16L * 1024 * 1024 * 1024))
    }
}
