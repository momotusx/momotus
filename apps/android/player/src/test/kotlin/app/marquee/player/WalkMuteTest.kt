package app.marquee.player

import android.content.Intent
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A walk's mute lasts across the walk's own launches and ends when the owner opens the app from its icon. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class WalkMuteTest {
    private val context = RuntimeEnvironment.getApplication()

    @Test
    fun `a walk's mute holds for its later launches and the launcher clears it`() {
        WalkMute.read(context, Intent().putExtra("mute", true))
        assertTrue(WalkMute.on)
        // `am start -n` and links carry no mute word: the walk stays muted.
        WalkMute.read(context, Intent())
        assertTrue(WalkMute.on)
        WalkMute.read(context, Intent(Intent.ACTION_VIEW))
        assertTrue(WalkMute.on)
        // The owner taps the icon.
        WalkMute.read(context, Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER))
        assertFalse(WalkMute.on)
        WalkMute.read(context, Intent())
        assertFalse(WalkMute.on)
    }
}
