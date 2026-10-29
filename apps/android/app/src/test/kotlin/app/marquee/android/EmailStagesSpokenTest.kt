package app.marquee.android

import android.app.Application
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import app.marquee.core.AccountCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.session.CoreOps
import app.marquee.core.sync.EmailCode
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * What TalkBack hears on the email screen (plans/accounts-email.md 4, flow 3 and 4): each stage's title names
 * its pane, `codeResent` is said once in a polite live region after a resend that worked, and the countdown on
 * Send a new code is not a live region.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class EmailStagesSpokenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    // The form's work runs on the main looper, as the app's session scope reaches the composition.
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

    @Volatile private var clock = 1_000L

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    /** A form over a core whose every `emailSignInStart` mails a code. */
    private fun form(): EmailCode {
        client.attach(
            CoreTransport { text ->
                val id =
                    CoreJson
                        .parseToJsonElement(text)
                        .jsonObject
                        .getValue("id")
                        .jsonPrimitive.long
                client.onMessage("""{"id":$id,"ok":true,"result":{"flow":"F","expiresMs":${Long.MAX_VALUE}}}""")
            },
        )
        return EmailCode(
            CoreOps(client),
            { "https://sync.example.org" },
            { work -> scope.launch { work() } },
            { },
            now = { clock },
        ).also { it.open() }
    }

    private fun until(condition: () -> Boolean) =
        compose.waitUntil(5_000) {
            shadowOf(Looper.getMainLooper()).idle()
            compose.waitForIdle()
            condition()
        }

    /** The native live regions under the window and their words, once every pending write is done. */
    private fun liveRegions(): List<Pair<Int, String>> {
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(2_000))
        val found = mutableListOf<Pair<Int, String>>()

        fun walk(view: View) {
            if (view is TextView && view.accessibilityLiveRegion != View.ACCESSIBILITY_LIVE_REGION_NONE) {
                found += view.accessibilityLiveRegion to view.text.toString()
            }
            if (view is ViewGroup) (0 until view.childCount).forEach { walk(view.getChildAt(it)) }
        }
        walk(compose.activity.window.decorView)
        return found
    }

    private fun pane(title: String) = SemanticsMatcher.expectValue(SemanticsProperties.PaneTitle, title)

    private fun atCode(f: EmailCode) {
        compose.setContent { MarqueeTheme(Form.Phone) { EmailStages(f) { } } }
        compose.onNode(pane(AccountCopy.EMAIL_TITLE)).assertExists()
        f.address = "zoe@example.org"
        f.send()
        until { f.stage == EmailCode.Stage.Code && !f.busy }
    }

    @Test
    fun `each stage's title names its pane`() {
        val f = form()
        atCode(f)
        compose.onNode(pane(AccountCopy.CODE_TITLE)).assertExists()
        compose.onNode(pane(AccountCopy.EMAIL_TITLE)).assertDoesNotExist()
    }

    @Test
    fun `the countdown is not a live region, and nothing is said before a resend`() {
        val f = form()
        atCode(f)
        val label = "Send a new code in 60 seconds"
        compose
            .onNodeWithText(label)
            .assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.LiveRegion))
        val live = liveRegions()
        assertTrue(live.none { it.second.startsWith("Send a new code in") }, "$live")
        assertTrue(live.none { it.second == AccountCopy.CODE_RESENT }, "$live")
    }

    @Test
    fun `codeResent is said once, politely, after a resend that worked`() {
        val f = form()
        atCode(f)
        clock += 60_000
        f.resend()
        until { f.resent && !f.busy }
        val said = liveRegions().filter { it.second == AccountCopy.CODE_RESENT }
        assertEquals(listOf(View.ACCESSIBILITY_LIVE_REGION_POLITE to AccountCopy.CODE_RESENT), said)
    }
}
