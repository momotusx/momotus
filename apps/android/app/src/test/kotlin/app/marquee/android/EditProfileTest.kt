package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/**
 * The profile editor (design/DECISION.md, Profiles; plans/2026-10-28-ui-ux-audit.md, S8): one save at a time, a draft
 * that survives recreation, a question before a changed page is left or a PIN removed unseen, no Color row.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h2400dp")
class EditProfileTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)
    private val ops = CopyOnWriteArrayList<String>()

    /** The answer to a held `createProfile`, once the test lets it through. */
    @Volatile private var held: (() -> Unit)? = null

    @Volatile private var holdCreate = false

    @After
    fun stop() {
        session.scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
        session.client.close()
    }

    private val ana = profile("p1", "Ana", owner = true, pin = false)
    private val bo = profile("p2", "Bo", owner = false, pin = true)
    private val snapshot = ProfilesSnapshot(listOf(ana, bo), active = "p1", locked = false, max = 5, kids = false)

    private fun profile(
        id: String,
        name: String,
        owner: Boolean,
        pin: Boolean,
    ) = ProfileInfo(id, name, "letter:ember", 0, hasPin = pin, isOwner = owner, syncing = false, kids = false)

    /** A core that has [snapshot] for the profiles view and answers every write with it. */
    private fun core() {
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), snapshot)
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val op = msg["op"]?.jsonPrimitive?.content
            val reply = {
                val result = if (op == "subscribe") """{"sub":3,"snapshot":$json}""" else json
                session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
            }
            if (op != "subscribe") ops += op.orEmpty()
            if (op == "createProfile" && holdCreate) held = reply else reply()
        }
        session.opened(running = true)
        gate.start()
    }

    private fun until(
        what: String,
        done: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + 60_000
        while (!done()) {
            check(System.currentTimeMillis() < deadline) { "never: $what" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun edit(
        id: String?,
        onDone: () -> Unit = {},
    ) {
        core()
        compose.setContent { MarqueeTheme(Form.Phone) { EditProfileScreen(gate, session, id, onDone) } }
        until("the page") { compose.onAllNodesWithContentDescription("Name").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun name() = compose.onNodeWithContentDescription("Name")

    @Test
    fun `a second tap on Add profile while the first is with the core adds nothing more`() {
        holdCreate = true
        var done = 0
        edit(null) { done++ }
        name().performTextInput("Cy")
        compose.onNode(hasText("Add profile") and hasClickAction()).performClick()
        until("the first create") { ops.contains("createProfile") }
        compose.onNode(hasText("Add profile") and hasClickAction()).performClick()
        compose.waitForIdle()
        held!!.invoke()
        until("the page left") { done == 1 }
        assertEquals(1, ops.count { it == "createProfile" })
    }

    @Test
    fun `a draft survives recreation, its name and its picture`() {
        core()
        val restoration = StateRestorationTester(compose)
        restoration.setContent { MarqueeTheme(Form.Phone) { EditProfileScreen(gate, session, null, {}) } }
        until("the page") { compose.onAllNodesWithContentDescription("Name").fetchSemanticsNodes().isNotEmpty() }
        name().performTextInput("Cy")
        compose.onNodeWithContentDescription("Rocket").performClick()
        restoration.emulateSavedInstanceStateRestore()
        until("the page again") { compose.onAllNodesWithContentDescription("Name").fetchSemanticsNodes().isNotEmpty() }
        name().assertTextEquals("Cy", includeEditableText = true)
        compose
            .onNode(
                SemanticsMatcher("Rocket is selected") {
                    it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains("Rocket") == true &&
                        it.config.getOrNull(SemanticsProperties.Selected) == true
                },
            ).assertExists()
    }

    private fun back() {
        // The guard is armed by the recomposition after the typing: Back is pressed once that has run.
        compose.waitForIdle()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
    }

    @Test
    fun `Back with a changed name asks first, Keep editing keeps it, Discard leaves`() {
        var done = 0
        edit(null) { done++ }
        name().performTextInput("Cy")
        back()
        compose.waitForIdle()
        compose.onNodeWithText("Discard this profile?").assertExists()
        compose.onNodeWithText("Keep editing").assertIsFocused().performClick()
        compose.onAllNodesWithText("Discard this profile?").assertCountEquals(0)
        name().assertTextEquals("Cy", includeEditableText = true)
        assertEquals(0, done)
        back()
        compose.waitForIdle()
        compose.onNodeWithText("Discard").performClick()
        assertEquals(1, done)
    }

    @Test
    fun `the phone's back link goes through the dispatcher, so a changed page is asked first`() {
        core()
        var popped = 0
        compose.setContent {
            // As AppRoot wires it: the link is the system Back, and only an unhandled one pops.
            val systemBack = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
            MarqueeTheme(Form.Phone) {
                PhoneShell(
                    Route.Home,
                    showTabs = false,
                    onSelect = {},
                    onBack = { systemBack?.onBackPressed() ?: popped++ },
                ) { EditProfileScreen(gate, session, null, {}) }
            }
        }
        until("the page") { compose.onAllNodesWithContentDescription("Name").fetchSemanticsNodes().isNotEmpty() }
        name().performTextInput("Cy")
        compose.onNodeWithText("Back").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Discard this profile?").assertExists()
        assertEquals(0, popped)
    }

    @Test
    fun `Back on an untouched page goes straight back`() {
        edit("p2")
        back()
        compose.waitForIdle()
        compose.onAllNodesWithText("Discard your changes?").assertCountEquals(0)
        // No guard stands: a Back now goes to the system, which leaves the page.
        assertFalse(compose.activity.onBackPressedDispatcher.hasEnabledCallbacks())
    }

    @Test
    fun `Keep editing sends focus to Save, not to the name field and its keyboard`() {
        edit("p1")
        name().performTextInput("Ana Maria")
        back()
        compose.waitForIdle()
        compose.onNodeWithText("Keep editing").assertIsFocused().performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText("Discard your changes?").assertCountEquals(0)
        compose.onNodeWithText("Save").assertIsFocused()
        name().assertIsNotFocused()
    }

    @Test
    fun `Keep on a PIN question gives focus back to the button that asked`() {
        edit("p2")
        compose.onNodeWithText("Remove Bo's PIN").performClick()
        compose.onNodeWithText("Keep").assertIsFocused().performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText("Keep").assertCountEquals(0)
        compose.onNodeWithText("Remove Bo's PIN").assertIsFocused()
    }

    @Test
    fun `the page has no Color row`() {
        edit(null)
        compose.onAllNodesWithText("PICTURE").assertCountEquals(1)
        compose.onAllNodesWithText("COLOR").assertCountEquals(0)
    }

    @Test
    fun `another profile's PIN goes after a question, and Keep sends nothing`() {
        edit("p2")
        compose.onNodeWithText("Remove Bo's PIN").performClick()
        compose.onNodeWithText("Keep").assertIsFocused().performClick()
        compose.waitForIdle()
        assertEquals(0, ops.count { it == "setPin" })
        compose.onNodeWithText("Remove Bo's PIN").performClick()
        compose.onNodeWithText("Remove PIN").performClick()
        until("the PIN removal") { ops.contains("setPin") }
        assertEquals(1, ops.count { it == "setPin" })
    }
}
