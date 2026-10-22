package app.marquee.android

import android.content.Intent
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A TV-form walk on a phone asks for landscape itself, and only a debug build listens. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = android.app.Application::class)
class WalkLandscapeTest {
    @Test
    fun `a debug build asked for landscape takes it, a release build or a plain launch does not`() {
        val asked = Intent().putExtra("form", "tv").putExtra("landscape", true)
        assertTrue(asked.walkLandscape(debuggable = true))
        assertFalse(asked.walkLandscape(debuggable = false))
        assertFalse(Intent().putExtra("form", "tv").walkLandscape(debuggable = true))
    }
}
