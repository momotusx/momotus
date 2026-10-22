package app.marquee.ui.phone

import android.graphics.Bitmap
import android.graphics.Canvas
import app.marquee.player.R
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Every circle-flags drawable the player bundles (scripts/flags.ts) inflates and draws with Android's own vector
 * parser: a round flag, filled in the middle and clear in the corners.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36])
class FlagsDrawTest {
    @Test
    fun `each flag draws as a filled circle`() {
        val context = RuntimeEnvironment.getApplication()
        val flags = R.drawable::class.java.fields.filter { it.name.startsWith("flag_") }
        assertTrue(flags.size > 190, "circle-flags' language set: ${flags.size}")
        val bad =
            flags.mapNotNull { field ->
                val drawable = context.getDrawable(field.getInt(null)) ?: return@mapNotNull "${field.name}: no drawable"
                val bitmap = Bitmap.createBitmap(48, 48, Bitmap.Config.ARGB_8888)
                drawable.setBounds(0, 0, 48, 48)
                drawable.draw(Canvas(bitmap))
                val middle = bitmap.getPixel(24, 24) ushr 24
                val corner = bitmap.getPixel(1, 1) ushr 24
                "${field.name}: middle $middle, corner $corner".takeIf { middle < 250 || corner > 0 }
            }
        assertEquals(emptyList(), bad)
    }

    @Test
    fun `the flags' MIT notice ships with them`() {
        val notice =
            RuntimeEnvironment
                .getApplication()
                .assets
                .open(
                    "licenses/circle-flags-MIT.md",
                ).bufferedReader()
                .readText()
        assertTrue("MIT License" in notice && "HatScripts" in notice, notice.take(80))
    }
}
