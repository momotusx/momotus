package app.marquee.android

import android.app.Application
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.DownloadCopy
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.session.CoreSession
import app.marquee.core.session.DownloadAction
import app.marquee.core.session.DownloadTarget
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** What the download pages say when an op did not go through, and how long (code review r2 n3), and the ask's rule. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class DownloadActionsTest {
    private val session = CoreSession(RuntimeEnvironment.getApplication())

    // The main looper's clock: the line's 6 s pass at once.
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val actions = DownloadActions(session, { error("no add here") }, scope)

    @Volatile private var refuse = true

    @After
    fun stop() {
        scope.cancel()
        session.scope.cancel()
        session.client.close()
    }

    private fun core() =
        session.client.attach(
            CoreTransport { text ->
                val id =
                    CoreJson
                        .parseToJsonElement(text)
                        .jsonObject
                        .getValue("id")
                        .jsonPrimitive.long
                val answer = if (refuse) """"ok":false,"error":"no"""" else """"ok":true,"result":{}"""
                session.client.onMessage("""{"id":$id,$answer}""")
            },
        )

    private fun until(ready: () -> Boolean) {
        val deadline = System.currentTimeMillis() + 10_000
        while (!ready()) {
            check(System.currentTimeMillis() < deadline) { "the op never answered" }
            shadowOf(Looper.getMainLooper()).idle()
            Snapshot.sendApplyNotifications()
            Thread.sleep(5)
        }
    }

    @Test
    fun `an op that did not go through says so for 6 seconds`() {
        core()
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until { actions.problem != null }
        assertEquals(DownloadCopy.ACT_FAILED, actions.problem)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(PROBLEM_MS - 100))
        assertEquals(DownloadCopy.ACT_FAILED, actions.problem)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(200))
        assertNull(actions.problem)
    }

    @Test
    fun `the next op that goes through, or another page, clears the line`() {
        core()
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until { actions.problem != null }
        refuse = false
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until { actions.problem == null }
        refuse = true
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until { actions.problem != null }
        // Navigation clears it (AppRoot, on every new top page).
        actions.clearProblem()
        assertNull(actions.problem)
        // A second failure starts its own 6 s: the first one's timer does not cut it short.
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until { actions.problem != null }
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(PROBLEM_MS - 100))
        assertEquals(DownloadCopy.ACT_FAILED, actions.problem)
    }

    /** Once, after a download started, on Android 13 and later, never when notifications are allowed (3.3). */
    @Test
    fun `the notification ask's rule`() {
        assertTrue(asksForNotifications(sdk = 33, starts = 1, asked = false, granted = false))
        assertFalse(asksForNotifications(sdk = 32, starts = 1, asked = false, granted = false))
        assertFalse(asksForNotifications(sdk = 36, starts = 0, asked = false, granted = false))
        assertFalse(asksForNotifications(sdk = 36, starts = 3, asked = true, granted = false))
        assertFalse(asksForNotifications(sdk = 36, starts = 3, asked = false, granted = true))
    }
}
