package app.marquee.ui.phone

import android.os.Looper
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import app.marquee.core.client.StateThreadRule
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * A snapshot the state thread applies is composed on the main thread, never on the state thread. The deprecated compose
 * rule ran composition on an unconfined test dispatcher: an apply on the state thread resumed the recomposer there,
 * and when the main thread advanced the test clock at that moment the frame's delay ended at once, so the state thread
 * composed and laid out views beside the main thread. That was the CalledFromWrongThreadException, the broken slot
 * table, "performMeasureAndLayout called during measure layout" and the screens that never drew, a different UI test
 * each CI run (2026-11). With that rule this test failed within 3000 applies in every run; the v2 rule queues the
 * resumption for the main thread.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class ComposeThreadTest {
    @get:Rule
    val compose = createComposeRule()

    @get:Rule
    val state = StateThreadRule()

    @Test
    fun `a snapshot applied on the state thread composes on the main thread`() {
        var count by mutableIntStateOf(0)
        val composedOn = CopyOnWriteArrayList<Boolean>()
        compose.setContent {
            BasicText("count $count")
            SideEffect { composedOn += Looper.myLooper() == Looper.getMainLooper() }
        }
        compose.waitForIdle()
        val scope = CoroutineScope(state.dispatcher)
        for (next in 1..3000) {
            // The client applies while the main thread goes on with frames, as in every UI test.
            scope.launch { Snapshot.withMutableSnapshot { count = next } }
            val deadline = System.currentTimeMillis() + 10_000
            while (compose.onAllNodesWithText("count $next").fetchSemanticsNodes().isEmpty()) {
                check(System.currentTimeMillis() < deadline) { "count $next never drew" }
                compose.waitForIdle()
                Thread.sleep(1)
            }
        }
        assertEquals(emptyList(), composedOn.withIndex().filter { !it.value }.map { it.index }, "compositions off main")
    }

    @Test
    fun `no UI test builds its compose rule with the deprecated unconfined factory`() {
        val deprecated =
            Regex("""^import androidx\.compose\.ui\.test\.(junit4\.)?(create\w*ComposeRule|run\w*ComposeUiTest)$""")
        val files =
            listOf("app", "core", "design", "player", "ui-phone", "ui-tv")
                .flatMap { m -> File("..", "$m/src/test").walkTopDown().filter { it.extension == "kt" }.toList() }
        assertTrue(files.size > 100, "found ${files.size} tests")
        val hits = files.flatMap { f -> f.readLines().filter { deprecated.matches(it) }.map { "${f.path}: $it" } }
        assertEquals(emptyList(), hits)
    }
}
