package app.marquee.android

import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.session.CoreSession
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.concurrent.thread
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The start decision itself (`AppState.decide`): it hears the core open even when the app state is built inside a
 * composition's snapshot that the boot thread's write lands behind, and a core that cannot open sends it Home at
 * once instead of at the 10 s limit.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class StartDecisionTest {
    private val logs = CopyOnWriteArrayList<String>()
    private val caps: CapsSource = { error("no Versions view opens here") }

    @Before
    fun capture() {
        CoreLog.sink = { _, message, _ -> logs.add(message) }
    }

    @After
    fun release() {
        CoreLog.sink = { _, _, _ -> }
    }

    @Test
    fun `the start hears the core open while the composition that builds it runs`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        val composition = Snapshot.takeMutableSnapshot()
        composition.enter {
            // The boot thread opens the core, and the change is sent before the start listens.
            thread {
                session.opened(running = true)
                Snapshot.sendApplyNotifications()
            }.join()
            AppState(session, caps, shutsBefore = 0)
        }
        composition.apply().check()
        composition.dispose()
        shadowOf(Looper.getMainLooper()).idle()
        assertTrue("start: the core runs" in logs, "$logs")
    }

    @Test
    fun `a core that cannot open sends the start Home at once`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        session.ready.completeExceptionally(CoreError("The core could not start"))
        val state = AppState(session, caps, shutsBefore = 0)
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(Start.Home, state.start)
        assertTrue(logs.none { it.startsWith("start: no decision") }, "$logs")
    }
}
