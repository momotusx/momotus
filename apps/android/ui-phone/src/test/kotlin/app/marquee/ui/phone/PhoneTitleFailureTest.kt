package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
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

/** A title whose subscription fails before any header arrived says so with Try again, never skeletons for good. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h2400dp")
class PhoneTitleFailureTest {
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
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(
                    view,
                    null,
                    onPlay = { _, _ -> },
                    onVersions = {},
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.onAllNodesWithText("This title could not load", useUnmergedTree = true).assertCountEquals(0)
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
        compose.onNodeWithText("The core did not open", useUnmergedTree = true).assertExists()
        compose.onNodeWithText("Try again").assertHasClickAction()
    }
}
