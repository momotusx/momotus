package app.marquee.player

import android.view.accessibility.CaptioningManager
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowLooper
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The player follows the system caption settings while it runs (the Pixel walk, 2026-11-01: only after a restart). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class CaptionsTest {
    @Test
    fun `a change of the system captions reaches the player at once, until it stops following`() {
        val context = RuntimeEnvironment.getApplication()
        val manager = context.getSystemService(CaptioningManager::class.java)
        val seen = mutableListOf<Captions>()
        val stop = followCaptions(context) { seen += it }
        shadowOf(manager).setEnabled(true)
        ShadowLooper.idleMainLooper()
        assertTrue(seen.isNotEmpty(), "the change never came")
        assertEquals(captions(context).scale, seen.last().scale)
        stop()
        val before = seen.size
        shadowOf(manager).setEnabled(false)
        ShadowLooper.idleMainLooper()
        assertEquals(before, seen.size, "still following after stop")
    }
}
