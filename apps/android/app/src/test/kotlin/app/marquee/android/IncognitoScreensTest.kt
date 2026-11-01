package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreJson
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.LocalIncognito
import app.marquee.design.MarqueeTheme
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
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
import kotlin.test.assertTrue

/**
 * Incognito's way in and out on Android (plans/batch-10-incognito.md 5.1 to 5.4, 7.2): the chooser's tile, the bar's
 * menu, the keypad, the intro, the chooser's lines after an end, Settings' header, and Recents.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w960dp-h540dp")
class IncognitoScreensTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)

    /** Every request the app sent, whole. */
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @After
    fun stop() = session.client.close()

    private fun profile(
        id: String,
        pin: Boolean = false,
        kids: Boolean = false,
    ) = ProfileInfo(
        id,
        id.replaceFirstChar { it.uppercase() },
        "letter:ember",
        0,
        hasPin = pin,
        isOwner = id == "ana",
        syncing = false,
        kids = kids,
    )

    private val ana = profile("ana", pin = true)
    private val bo = profile("bo")

    private fun snapshot(
        host: String? = null,
        pin: String? = null,
        locked: Boolean = false,
        kids: Boolean = false,
        profiles: List<ProfileInfo> = listOf(ana, bo),
        active: String = "ana",
        ended: Boolean = false,
    ) = ProfilesSnapshot(
        profiles,
        if (host != null) ProfileCopy.INCOGNITO else active,
        locked,
        5,
        kids,
        incognitoHost = host,
        incognitoPin = pin,
        incognitoEnded = ended,
    )

    private fun ops() = sent.mapNotNull { it["op"]?.jsonPrimitive?.content }

    /** The core answers every request with [s]; [form] and [content] are the screen under test. */
    private fun open(
        s: ProfilesSnapshot,
        form: Form = Form.Tv,
        content: @Composable () -> Unit,
    ) {
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), s)
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            sent += msg
            val op = msg["op"]?.jsonPrimitive?.content ?: ""
            val result = if (op == "subscribe") """{"sub":3,"snapshot":$json}""" else json
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        session.opened(running = true)
        gate.start()
        compose.setContent { MarqueeTheme(form) { content() } }
        waitUntil("a profiles snapshot") { gate.snapshot != null }
        compose.waitForIdle()
    }

    private fun waitUntil(
        what: String,
        check: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + 60_000
        while (!check()) {
            check(System.currentTimeMillis() < deadline) { "never: $what" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    /** The app let the old profile go: the switch or Leave the gate holds goes to the core. */
    private fun resetAndWait(op: String) {
        compose.runOnUiThread { gate.resetDone(gate.shutCount) }
        waitUntil(op) { op in ops() }
    }

    private fun menuRows(): List<String> =
        compose
            .onAllNodes(hasClickAction() and hasAnyAncestor(isDialog()))
            .fetchSemanticsNodes()
            .sortedBy { it.boundsInRoot.top }
            .map { n ->
                n.config
                    .getOrElse(SemanticsProperties.ContentDescription) {
                        n.config.getOrElse(SemanticsProperties.Text) { emptyList() }.map { it.text }
                    }.joinToString()
            }

    private fun row(said: String) = compose.onNode((hasContentDescription(said) or hasText(said)) and hasClickAction())

    private val pins = mutableListOf<String>()

    private fun menu(s: ProfilesSnapshot) {
        open(s) { PillProfile(gate, false, Modifier, {}, { pins += it }) }
        val me =
            app.marquee.core.AccountRules
                .activeProfile(s)
                ?.name
        compose.onNodeWithContentDescription("Profile: $me").performClick()
        compose.waitForIdle()
    }

    @Test
    fun `the menu has the same rows in both states, Leave right under the checked Incognito, focus never on Leave`() {
        menu(snapshot(host = "ana"))
        assertEquals(
            listOf("Ana, locked with a PIN", "Bo", "Incognito, current profile", "Leave Incognito", "Settings"),
            menuRows(),
        )
        // TV focus starts on the current row: Incognito's, the row after the profiles, never Leave under it.
        assertEquals(2, menuStart(snapshot(host = "ana")))
        assertEquals(1, menuStart(snapshot(active = "bo")))
        row("Leave Incognito").performClick()
        // Back to where it was entered, with no PIN: the gate shuts for the host and Leave goes once the views closed.
        waitUntil("the gate shut for the host") { gate.switching == "ana" }
        resetAndWait("leaveIncognito")
        assertEquals(1, ops().count { it == "leaveIncognito" })
    }

    @Test
    fun `outside it the row switches straight in, from the menu, and the intro waits`() {
        menu(snapshot())
        assertEquals(listOf("Ana, current profile", "Bo", "Incognito", "Settings"), menuRows())
        row("Incognito").performClick()
        waitUntil("the gate shut for Incognito") { gate.switching == ProfileCopy.INCOGNITO }
        resetAndWait("switchProfile")
        val switch = sent.last { it["op"]?.jsonPrimitive?.content == "switchProfile" }
        assertEquals("incognito", switch["profile"]?.jsonPrimitive?.content)
        assertEquals(null, switch["fromChooser"])
        waitUntil("the intro is due") { gate.intro }
    }

    @Test
    fun `a PIN the core asks for opens its keypad from the menu`() {
        menu(snapshot(pin = "ana"))
        row("Incognito, needs a PIN").performClick()
        assertEquals(listOf(ProfileCopy.INCOGNITO), pins)
        assertFalse("switchProfile" in ops())
    }

    @Test
    fun `a kids session has no Incognito row`() {
        menu(snapshot(kids = true, active = "bo"))
        assertEquals(listOf("Ana, locked with a PIN", "Bo, current profile", "Settings"), menuRows())
    }

    private fun tiles(): List<String> =
        compose
            .onAllNodes(hasClickAction())
            .fetchSemanticsNodes()
            .sortedWith(compareBy({ it.boundsInRoot.top }, { it.boundsInRoot.left }))
            .mapNotNull { it.config.getOrElse(SemanticsProperties.ContentDescription) { emptyList() }.firstOrNull() }

    @Test
    fun `the chooser's Incognito tile sits after the profiles and before Add profile, and goes in from the chooser`() {
        open(snapshot(active = "ana")) { PickerScreen(gate, session, PickerMode.Start, onAdd = {}) }
        assertEquals(listOf("Ana, current profile, locked with a PIN", "Bo", "Incognito", "Add profile"), tiles())
        compose.onNodeWithContentDescription("Incognito").performClick()
        compose.waitForIdle()
        resetAndWait("switchProfile")
        val switch = sent.last { it["op"]?.jsonPrimitive?.content == "switchProfile" }
        assertEquals(true, switch["fromChooser"]?.jsonPrimitive?.booleanOrNull)
    }

    @Test
    fun `the tile opens the keypad the core names, and says why only under the kids rule`() {
        val kid = profile("kid", kids = true)
        open(snapshot(pin = "ana", active = "bo", profiles = listOf(ana, bo, kid))) {
            PickerScreen(gate, session, PickerMode.Start)
        }
        compose.onNodeWithContentDescription("Incognito, needs a PIN").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Enter the PIN for Ana to open Incognito").assertExists()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_KIDS_REASON).assertExists()
    }

    @Test
    fun `a locked session's keypad gives no kids reason`() {
        open(snapshot(pin = "ana", locked = true, active = "bo")) { PickerScreen(gate, session, PickerMode.Locked) }
        compose.onNodeWithContentDescription("Incognito, needs a PIN").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Enter the PIN for Ana to open Incognito").assertExists()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_KIDS_REASON).assertDoesNotExist()
    }

    @Test
    fun `the pushed picker in Incognito starts on the Incognito tile`() {
        open(snapshot(host = "bo")) { PickerScreen(gate, session, PickerMode.Switch) }
        compose.mainClock.advanceTimeBy(2_000)
        compose.onNodeWithContentDescription("Incognito, current profile").assertIsFocused()
    }

    @Test
    fun `after a Leave to the chooser a host with no PIN shows no lock line, and an end shows its line`() {
        open(snapshot(locked = true, active = "bo", ended = true)) { PickerScreen(gate, session, PickerMode.Locked) }
        compose.onNodeWithText("This profile is locked", substring = true).assertDoesNotExist()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_ENDED).assertExists()
    }

    @Test
    fun `a locked host with a PIN still says it is locked, and no end means no ended line`() {
        open(snapshot(locked = true, active = "ana")) { PickerScreen(gate, session, PickerMode.Locked) }
        compose.onNodeWithText("This profile is locked", substring = true).assertExists()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_ENDED).assertDoesNotExist()
    }

    @Test
    fun `the TV intro says what is kept, Continue has the focus, and Back leaves`() {
        var left = 0
        var went = 0
        open(snapshot(host = "ana")) { IncognitoIntro(debrid = true, onContinue = { went++ }, onLeave = { left++ }) }
        compose.onNodeWithText("You're Incognito").assertExists()
        compose.onNodeWithText("What you watch and search.").assertExists()
        compose.onNodeWithText("Your debrid account keeps its own history.").assertExists()
        compose.onNodeWithText("It ends when you leave, or after 30 minutes away.").assertExists()
        compose.mainClock.advanceTimeBy(2_000)
        val go = compose.onNode(hasText("Continue") and hasClickAction())
        go.assertIsFocused()
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        assertEquals(1, left)
        go.performClick()
        assertEquals(1, went)
    }

    @Test
    fun `the phone intro has the phone's end line and no debrid line without an account`() {
        open(snapshot(host = "ana"), Form.Phone) { IncognitoIntro(debrid = false, onContinue = {}, onLeave = {}) }
        compose.onNodeWithText("It ends when you leave, close the app, or after 30 minutes away.").assertExists()
        compose.onNodeWithText("Your debrid account keeps its own history.").assertDoesNotExist()
        compose.onNode(hasText(ProfileCopy.INCOGNITO_LEAVE) and hasClickAction()).assertExists()
    }

    @Test
    fun `the ended notice over Home stays until OK, which tells the core it was read`() {
        open(snapshot(ended = true), Form.Phone) { IncognitoEndedCard(gate::ackEnded) }
        compose.onNodeWithText(ProfileCopy.INCOGNITO_ENDED).assertExists()
        assertFalse("ackIncognitoEnded" in ops())
        compose.onNode(hasText("OK") and hasClickAction()).performClick()
        waitUntil("the ack") { "ackIncognitoEnded" in ops() }
    }

    @Test
    fun `Settings in Incognito hides the profiles, the account and Downloads, and keeps the rest`() {
        val shown = SettingsSection.entries.filter { incognitoShows(true, it) }
        assertEquals(
            listOf(
                SettingsSection.Sources,
                SettingsSection.Home,
                SettingsSection.Playback,
                SettingsSection.Subtitles,
                SettingsSection.Device,
                SettingsSection.About,
            ),
            shown,
        )
        assertTrue(SettingsSection.entries.all { incognitoShows(false, it) })
    }

    @Test
    fun `TV Settings in Incognito starts on its block, with Leave beside it`() {
        var left = 0
        var chooser = 0
        open(snapshot(host = "ana")) {
            TvSettings(
                ProfileCopy.incognito,
                ProfileCopy.INCOGNITO_KEPT_NOTHING,
                null,
                canEdit = false,
                canManage = false,
                onProfiles = { chooser++ },
                onEdit = {},
                onManage = {},
                parts = listOf(SettingsPart(SettingsSection.About) {}),
                onLeave = { left++ },
            )
        }
        compose.mainClock.advanceTimeBy(2_000)
        compose.onNodeWithText(ProfileCopy.INCOGNITO_KEPT_NOTHING).assertExists()
        compose.onNodeWithTag(SETTINGS_PROFILE).assertIsFocused()
        compose.onNodeWithTag(SETTINGS_PROFILE).performClick()
        assertEquals(1, chooser)
        compose.onNode(hasText(ProfileCopy.INCOGNITO_LEAVE) and hasClickAction()).performClick()
        assertEquals(1, left)
        compose.onNodeWithText("Edit profile").assertDoesNotExist()
    }

    @Test
    fun `phone Settings in Incognito has Leave as the header's one button`() {
        var left = 0
        var chooser = 0
        open(snapshot(host = "ana"), Form.Phone) {
            PhoneSettings(
                ProfileCopy.incognito,
                ProfileCopy.INCOGNITO_KEPT_NOTHING,
                null,
                { chooser++ },
                emptyList(),
                onLeave = { left++ },
            )
        }
        compose.onNodeWithText("Incognito").assertExists()
        compose.onNode(hasText(ProfileCopy.INCOGNITO_LEAVE) and hasClickAction()).performClick()
        assertEquals(1, left)
        // The card itself opens nothing: the one button is Leave.
        compose.onNodeWithText("Incognito").performClick()
        assertEquals(0, chooser)
    }

    @Test
    fun `the trailer previews row keeps its switch and says it is off in Incognito`() {
        var incognito by mutableStateOf(true)
        open(snapshot(host = "ana"), Form.Phone) {
            CompositionLocalProvider(LocalIncognito provides incognito) { DevicePrefs() }
        }
        compose.onNodeWithText("Trailer previews").assertExists()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_TRAILERS_OFF).assertExists()
        incognito = false
        compose.waitForIdle()
        compose.onNodeWithText(ProfileCopy.INCOGNITO_TRAILERS_OFF).assertDoesNotExist()
    }

    @Test
    fun `closing the app for good leaves Incognito at once, with no screen left to reset`() {
        open(snapshot(host = "ana"), Form.Phone) {}
        gate.intro = true
        compose.runOnUiThread { gate.leaveOnClose() }
        waitUntil("leaveIncognito") { "leaveIncognito" in ops() }
        assertFalse(gate.intro)
        // The gate never shut for it: nothing waits for a reset that will not come.
        assertEquals(null, gate.switching)
    }

    @Test
    fun `closing the app outside Incognito sends nothing`() {
        open(snapshot(), Form.Phone) {}
        compose.runOnUiThread { gate.leaveOnClose() }
        compose.waitForIdle()
        Thread.sleep(200)
        assertFalse("leaveIncognito" in ops())
    }

    @Test
    fun `Recents keeps the last frame only outside Incognito, from Android 13`() {
        val set = mutableListOf<Boolean>()
        recentsFrame(33, incognito = true) { set += it }
        recentsFrame(33, incognito = false) { set += it }
        recentsFrame(32, incognito = true) { set += it }
        assertEquals(listOf(false, true), set)
    }
}
