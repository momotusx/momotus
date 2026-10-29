package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.client.CoreJson
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
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
import kotlin.test.assertEquals

/**
 * The profile's menu at the bar's end (plans/nav-profile.md): every profile, the current one checked, then Settings
 * and nothing else; a row does what its picker tile does.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w960dp-h540dp")
class ProfileMenuTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)
    private val ops = mutableListOf<String>()

    @After
    fun stop() = session.client.close()

    private fun profile(
        id: String,
        name: String,
        pin: Boolean = false,
        syncing: Boolean = false,
    ) = ProfileInfo(id, name, "letter:ember", 0, hasPin = pin, isOwner = id == "ana", syncing = syncing, kids = false)

    private fun open(content: @Composable () -> Unit) =
        openWith { compose.setContent { MarqueeTheme(Form.Tv) { content() } } }

    /** The core answers with four profiles; [setContent] composes the screen under test. */
    private fun openWith(setContent: () -> Unit) {
        val all =
            listOf(
                profile("ana", "Ana"),
                profile("kid", "Kid"),
                profile("bo", "Bo", pin = true),
                profile("cy", "Cy", syncing = true),
            )
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), ProfilesSnapshot(all, "ana", false, 5, false))
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val op = msg["op"]?.jsonPrimitive?.content ?: ""
            ops += op
            val result = if (op == "subscribe") """{"sub":3,"snapshot":$json}""" else json
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        session.opened(running = true)
        gate.start()
        setContent()
        val deadline = System.currentTimeMillis() + 60_000
        while (gate.snapshot == null) {
            check(System.currentTimeMillis() < deadline) { "no profiles snapshot" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.waitForIdle()
    }

    private var settings = 0
    private val pins = mutableListOf<String>()

    private fun showButton(lit: Boolean = false) =
        open { PillProfile(gate, lit, Modifier, { settings++ }, { pins += it }) }

    // A profile row says its spoken label alone; Settings says its word.
    private fun row(said: String) = compose.onNode((hasContentDescription(said) or hasText(said)) and hasClickAction())

    @Test
    fun `the menu lists the profiles, the current one first focused, then Settings, and nothing else`() {
        showButton()
        compose.onNodeWithContentDescription("Profile: Ana").performClick()
        compose.waitForIdle()
        compose.onNode(isDialog()).assertExists()
        val rows =
            compose
                .onAllNodes(hasClickAction())
                .fetchSemanticsNodes()
                .map { n ->
                    n.config
                        .getOrElse(SemanticsProperties.ContentDescription) {
                            n.config.getOrElse(SemanticsProperties.Text) { emptyList() }.map { it.text }
                        }.joinToString()
                }.filter { it != "Profile: Ana" }
        assertEquals(
            listOf("Ana, current profile", "Kid", "Bo, locked with a PIN", "Cy, still syncing", "Settings"),
            rows,
        )
        compose.onNodeWithText("Profiles").assertExists()
        // TV: focus starts on the current profile.
        compose.mainClock.advanceTimeBy(2_000)
        compose.onNode(hasContentDescription("Ana, current profile")).assertIsFocused()
    }

    @Test
    fun `Settings opens Settings, a PIN opens its keypad, and the current or a syncing profile only close`() {
        showButton(lit = true)
        val button = compose.onNodeWithContentDescription("Profile: Ana")
        button.performClick()
        row("Settings").performClick()
        compose.onNode(isDialog()).assertDoesNotExist()
        assertEquals(1, settings)
        button.performClick()
        row("Bo, locked with a PIN").performClick()
        assertEquals(listOf("bo"), pins)
        button.performClick()
        row("Ana, current profile").performClick()
        button.performClick()
        row("Cy, still syncing").performClick()
        compose.onNode(isDialog()).assertDoesNotExist()
        assertEquals(listOf("bo"), pins)
        assertEquals(null, gate.switching)
        assertEquals(0, ops.count { it == "switchProfile" })
    }

    @Test
    fun `a profile without a PIN switches, the gate shutting as the picker's tile does`() {
        showButton()
        compose.onNodeWithContentDescription("Profile: Ana").performClick()
        row("Kid").performClick()
        compose.onNode(isDialog()).assertDoesNotExist()
        // The switch began though the menu left at once.
        assertEquals("kid", gate.switching)
    }

    @Test
    fun `on a phone in an account, Sign in another device sits after the profiles, before Settings`() {
        var scans = 0
        open { ProfileMenu(gate, {}, {}, {}) { scans++ } }
        val said =
            compose
                .onAllNodes(hasClickAction())
                .fetchSemanticsNodes()
                .map { n ->
                    n.config
                        .getOrElse(SemanticsProperties.ContentDescription) {
                            n.config.getOrElse(SemanticsProperties.Text) { emptyList() }.map { it.text }
                        }.joinToString()
                }
        assertEquals(listOf("Sign in another device", "Settings"), said.takeLast(2))
        row("Sign in another device").performClick()
        assertEquals(1, scans)
    }

    @Test
    fun `the picker the menu opens shows that profile's keypad, and Cancel leaves the picker`() {
        var left = 0
        var round by mutableIntStateOf(0)
        gate.asked = "bo"
        open { key(round) { PickerScreen(gate, session, PickerMode.Switch, onBack = { left++ }) } }
        compose.onNodeWithText("Enter the PIN for Bo").assertExists()
        assertEquals(null, gate.asked)
        compose.onNodeWithText("Cancel").performClick()
        compose.waitForIdle()
        assertEquals(1, left)
        // Asked once: a picker opened later starts on the tiles.
        round++
        compose.waitForIdle()
        compose.onNodeWithText("Who's watching?").assertExists()
        compose.onNodeWithText("Enter the PIN for Bo").assertDoesNotExist()
    }

    @Test
    fun `the keypad the menu asked for survives a restored state, asked once`() {
        val restoration = StateRestorationTester(compose)
        gate.asked = "bo"
        openWith { restoration.setContent { MarqueeTheme(Form.Tv) { PickerScreen(gate, session, PickerMode.Switch) } } }
        compose.onNodeWithText("Enter the PIN for Bo").assertExists()
        restoration.emulateSavedInstanceStateRestore()
        compose.waitForIdle()
        compose.onNodeWithText("Enter the PIN for Bo").assertExists()
        compose.onNodeWithText("Cancel").assertExists()
        assertEquals(null, gate.asked)
    }

    @Test
    fun `the lit avatar says Settings shows, and the unlit one does not`() {
        var lit by mutableStateOf(false)
        open { PillProfile(gate, lit, Modifier, {}, {}) }
        val avatar = compose.onNodeWithContentDescription("Profile: Ana")
        avatar.assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.StateDescription))
        lit = true
        compose.waitForIdle()
        avatar.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, SETTINGS_SHOWN))
    }

    @Test
    fun `with no profile known, the bar's slot takes focus only while Settings shows`() {
        var lit by mutableStateOf(true)
        // No core answer: the gate has no snapshot.
        compose.setContent { MarqueeTheme(Form.Tv) { PillProfile(gate, lit, Modifier.testTag("slot"), {}, {}) } }
        val focusable = SemanticsMatcher.keyIsDefined(SemanticsProperties.Focused)
        compose.onNodeWithTag("slot").assert(focusable)
        lit = false
        compose.waitForIdle()
        compose.onNodeWithTag("slot").assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.Focused))
    }
}
