package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.view.View
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import app.marquee.core.ErrorCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertTrue

/** A title whose subscription fails before any header arrived says so with Try again, never skeletons for good. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvTitleFailureTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `a view that failed with no header shows the failure and Try again`() {
        client.attachFailed("The core did not open")
        val view = TitleView(client, scope, "movie", "tt1")
        var window: View? = null
        compose.setContent {
            window = LocalView.current
            MarqueeTheme(Form.Tv) {
                TvTitle(
                    view,
                    null,
                    FocusMemory(),
                    onPlay = { _, _ -> },
                    onVersions = {},
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.onAllNodesWithText("This title could not load", useUnmergedTree = true).assertCountEquals(0)
        // A touch device running the TV form (the debug walks): no focus is taken until the remote's mode is asked for.
        compose.runOnUiThread { enterTouchMode(window!!) }
        assertTrue(window!!.isInTouchMode, "the window is in touch mode")
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        // The words sit under a live region, which is read by TalkBack and hides them from the merged tree.
        val drawn = { compose.onAllNodesWithText("This title could not load", useUnmergedTree = true) }
        while (drawn().fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the failure never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        // The screen says the shared words; the core's own reason goes to the log only.
        compose.onNodeWithText(ErrorCopy.CORE_NOT_STARTED, useUnmergedTree = true).assertExists()
        compose.onAllNodesWithText("The core did not open", useUnmergedTree = true).assertCountEquals(0)
        compose.onNodeWithText("Try again").assertHasClickAction()
        // The remote has a place to start: Try again has the focus before any key is pressed.
        compose.waitUntil(10_000) {
            compose.onAllNodes(hasText("Try again") and isFocused()).fetchSemanticsNodes().isNotEmpty()
        }
    }

    /** Puts the window in touch mode, as a phone starts: the platform's own call, through reflection. */
    private fun enterTouchMode(view: View) {
        val root = View::class.java.getMethod("getViewRootImpl").invoke(view)
        val ensure = root.javaClass.getDeclaredMethod("ensureTouchMode", Boolean::class.javaPrimitiveType)
        ensure.isAccessible = true
        ensure.invoke(root, true)
    }
}
