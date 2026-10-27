package app.marquee.core

import android.os.Handler
import android.os.Looper
import app.marquee.core.client.StateThreadRule
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.model.Statement
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNotSame
import kotlin.test.assertTrue

/**
 * The state thread has a Looper of its own: Compose runs a view's layout command on the applying thread when that
 * thread's Looper is the view's handler's, and a view that is detaching has no handler, so a thread with no Looper
 * (null is null) touched views off the main thread (the CalledFromWrongThreadException seen in PhoneVersionsTest).
 * Its Looper loops, so a post there runs; the rule every UI test uses fails when anything but the dispatcher posts.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class StateThreadTest {
    private val nothing =
        object : Statement() {
            override fun evaluate() = Unit
        }

    @Test
    fun `the state thread has a Looper of its own, never the main one`() {
        val rule = StateThreadRule()
        val looper = runBlocking { withContext(rule.dispatcher) { Looper.myLooper() } }
        assertNotNull(looper, "a thread with no Looper runs a detaching view's commands itself")
        assertNotSame(Looper.getMainLooper(), looper)
        rule.apply(nothing, null).evaluate()
    }

    @Test
    fun `a post from anything but the dispatcher runs, and the rule fails the test`() {
        val rule = StateThreadRule()
        val ran = CountDownLatch(1)
        Handler(rule.looper).post { ran.countDown() }
        assertTrue(ran.await(5, TimeUnit.SECONDS), "the state Looper loops")
        val failure = assertFailsWith<AssertionError> { rule.apply(nothing, null).evaluate() }
        assertTrue("something else posted" in failure.message.orEmpty(), failure.message)
    }
}
