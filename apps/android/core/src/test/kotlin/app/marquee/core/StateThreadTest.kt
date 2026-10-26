package app.marquee.core

import android.os.Looper
import app.marquee.core.client.stateThread
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertNotNull
import kotlin.test.assertNotSame

/**
 * The state thread has a Looper of its own: Compose runs a view's layout command on the applying thread when that
 * thread's Looper is the view's handler's, and a view that is detaching has no handler, so a thread with no Looper
 * (null is null) touched views off the main thread (the CalledFromWrongThreadException seen in PhoneVersionsTest).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class StateThreadTest {
    @Test
    fun `the state thread has a Looper of its own, never the main one`() {
        val thread = stateThread("test-state")
        try {
            val looper = thread.submit<Looper?> { Looper.myLooper() }.get()
            assertNotNull(looper, "a thread with no Looper runs a detaching view's commands itself")
            assertNotSame(Looper.getMainLooper(), looper)
        } finally {
            thread.shutdownNow()
        }
    }
}
