package app.marquee.player

import android.graphics.Color
import android.view.accessibility.CaptioningManager
import androidx.media3.ui.CaptionStyleCompat
import app.marquee.core.model.SubtitleBackground
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowLooper
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertSame
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

    @Test
    fun `Box is today's look, Outline and Shadow draw a black edge on no box`() {
        assertEquals(CaptionStyleCompat.DEFAULT, captionStyle(null, SubtitleBackground.Box))
        assertEquals(CaptionStyleCompat.DEFAULT, captionStyle(null, SubtitleBackground.Unknown))
        val outline = captionStyle(null, SubtitleBackground.Outline)
        assertEquals(CaptionStyleCompat.EDGE_TYPE_OUTLINE, outline.edgeType)
        val shadow = captionStyle(null, SubtitleBackground.Shadow)
        assertEquals(CaptionStyleCompat.EDGE_TYPE_DROP_SHADOW, shadow.edgeType)
        for (style in listOf(outline, shadow)) {
            assertEquals(Color.TRANSPARENT, style.backgroundColor)
            assertEquals(Color.TRANSPARENT, style.windowColor)
            assertEquals(Color.WHITE, style.foregroundColor)
            assertEquals(Color.BLACK, style.edgeColor)
        }
    }

    @Test
    fun `a caption style the device has on wins over the profile's background`() {
        val context = RuntimeEnvironment.getApplication()
        val manager = context.getSystemService(CaptioningManager::class.java)
        assertNull(captions(context).system, "captions off: the profile's look")
        shadowOf(manager).setEnabled(true)
        val system = assertNotNull(captions(context).system)
        for (background in SubtitleBackground.entries) assertSame(system, captionStyle(system, background))
    }
}
