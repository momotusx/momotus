package app.marquee.core

import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.concurrent.thread
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The start decision hears the core open whenever it opens. A Pixel start waited the full 10 s with the core open
 * on 2026-10-22, and this race can do it: the decision's first read of the core's health runs inside a
 * composition's snapshot (the app state is built while composing), which still holds Starting; when the boot
 * thread's write and its apply notification go out before the read listens, nothing tells it again.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class CoreStartTest {
    @Test
    fun `the start hears the core open while its first read is inside a composition's snapshot`() =
        runBlocking {
            val session = CoreSession(RuntimeEnvironment.getApplication())
            val composition = Snapshot.takeMutableSnapshot()
            // The boot thread opens the core while the composition runs; the change is applied and sent at once.
            thread {
                session.opened(running = true)
                Snapshot.sendApplyNotifications()
            }.join()
            var decided: Job? = null
            composition.enter {
                // As `viewModelScope` on the main thread: the wait starts in the caller, inside the snapshot.
                decided = CoroutineScope(Dispatchers.Unconfined).launch { session.awaitOpen() }
            }
            composition.apply().check()
            composition.dispose()
            withTimeout(1_000) { decided!!.join() }
            assertEquals(CoreHealth.Running, session.health)
        }

    @Test
    fun `a wait on the core's health hears the boot thread with nothing else applying the change`() =
        runBlocking {
            // A fresh install on the Pixel (bench PGA12): the profiles gate waits on the health from the app's start,
            // before any composition, so nothing but the boot thread's own write can tell it the core runs.
            val session = CoreSession(RuntimeEnvironment.getApplication())
            val waiting =
                CoroutineScope(Dispatchers.Default).launch {
                    snapshotFlow { session.health }.first { it == CoreHealth.Running }
                }
            delay(200)
            thread { session.opened(running = true) }.join()
            withTimeout(2_000) { waiting.join() }
        }

    @Test
    fun `a core that died while it first opened keeps the start waiting for the next open`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        session.opened(running = false)
        assertFalse(session.ready.isCompleted, "the start waits for a core that runs")
        session.opened(running = true)
        assertTrue(session.ready.isCompleted)
    }
}
