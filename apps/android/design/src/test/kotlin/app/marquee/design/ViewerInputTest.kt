package app.marquee.design

import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Test
import kotlin.test.assertTrue

/** A preview waits for input to rest: a press during the wait starts the rest over. */
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
}
