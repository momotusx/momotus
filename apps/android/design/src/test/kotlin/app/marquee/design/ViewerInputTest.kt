package app.marquee.design

import android.view.MotionEvent
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** A preview waits for input to rest: a press during the wait starts the rest over. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ViewerInputTest {
    private val now = { System.nanoTime() / 1_000_000 }

    @After
    fun reset() {
        ViewerInput.at = 0L
    }

    @Test
    fun `no recent input starts at once, a press during the wait starts the rest over`() =
        runBlocking {
            ViewerInput.at = 0L
            val quick = now()
            ViewerInput.awaitIdle(now)
            assertTrue(now() - quick < 100, "waited with no input")

            val start = now()
            ViewerInput.at = start
            launch {
                delay(500)
                ViewerInput.at = now()
            }
            ViewerInput.awaitIdle(now)
            val waited = now() - start
            assertTrue(waited >= 500 + ViewerInput.IDLE_MS, "started $waited ms after the first press")
        }

    @Test
    fun `a drag held past the rest keeps it from starting until the finger lifts`() {
        val drag =
            listOf(
                MotionEvent.obtain(1_000, 1_000, MotionEvent.ACTION_DOWN, 0f, 0f, 0),
                MotionEvent.obtain(1_000, 2_200, MotionEvent.ACTION_MOVE, 0f, 50f, 0),
                MotionEvent.obtain(1_000, 3_100, MotionEvent.ACTION_UP, 0f, 90f, 0),
            )
        drag.forEach(ViewerInput::saw)
        assertEquals(3_100L, ViewerInput.at)
        drag.forEach(MotionEvent::recycle)
    }
}
