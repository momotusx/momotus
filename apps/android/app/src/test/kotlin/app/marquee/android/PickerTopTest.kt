package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.unit.dp
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.marquee.core.client.CoreJson
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.ui.phone.PhoneShell
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertTrue

/** On a short landscape phone the picker scrolls, and it starts under the status bar and the back link's row. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w800dp-h200dp")
class PickerTopTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)

    @After
    fun stop() = session.client.close()

    private fun open(content: @Composable () -> Unit) {
        val ana =
            ProfileInfo("p1", "Ana", "letter:ember", 0, hasPin = false, isOwner = true, syncing = false, kids = false)
        val snapshot = ProfilesSnapshot(listOf(ana), active = "p1", locked = false, max = 5, kids = false)
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), snapshot)
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") """{"sub":3,"snapshot":$json}""" else json
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        session.opened(running = true)
        gate.start()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Profiles, showTabs = false, onSelect = {}, onBack = {}, content = content)
            }
        }
        // A plain activity fits its content to the system bars, which would consume the inset before the page sees it.
        compose.runOnUiThread { WindowCompat.setDecorFitsSystemWindows(compose.activity.window, false) }
        compose.runOnIdle {
            ViewCompat.dispatchApplyWindowInsets(
                compose.activity.window.decorView,
                WindowInsetsCompat
                    .Builder()
                    .setInsets(WindowInsetsCompat.Type.statusBars(), Insets.of(0, 60, 0, 0))
                    .build(),
            )
        }
    }

    private fun until(
        what: String,
        matcher: SemanticsMatcher,
    ) {
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodes(matcher).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "$what never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun assertStartsUnderTheLink(
        what: String,
        matcher: SemanticsMatcher,
    ) {
        until(what, matcher)
        val top = compose.onNode(matcher).getBoundsInRoot().top
        val under = with(compose.density) { 60.toDp() } + 48.dp
        assertTrue(top >= under - 0.5.dp, "$what starts at $top, the link ends at $under")
    }

    @Test
    fun `the picker's first line starts under the status bar and the back link`() {
        open { PickerScreen(gate, session, PickerMode.Switch, onBack = {}) }
        assertStartsUnderTheLink("the first tile", hasContentDescription("Ana, current profile"))
    }

    @Test
    fun `the editor's PIN steps start under the status bar and the back link`() {
        open { EditProfileScreen(gate, session, "p1", {}) }
        // The form is longer than this short phone, so the PIN row is scrolled to before it is pressed.
        until("the editor", hasScrollAction())
        compose.onNode(hasScrollAction()).performScrollToNode(hasText("Set a PIN"))
        compose.onNodeWithText("Set a PIN").performClick()
        assertStartsUnderTheLink("the keypad's title", hasText("Choose a PIN"))
    }
}
