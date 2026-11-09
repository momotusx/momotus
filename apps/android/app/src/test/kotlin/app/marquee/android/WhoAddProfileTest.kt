package app.marquee.android

import android.app.Application
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.marquee.core.FirstRunCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.SyncCopy
import app.marquee.core.client.CoreJson
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.serialization.json.JsonObject
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
import kotlin.test.assertTrue

/**
 * Who's watching? over the wall (plans/first-screens.md 3.4): Add profile opens the create form inside the sheet, not
 * a page over Home; Cancel goes back to the tiles; an empty name is refused in place; Continue makes one profile with
 * the chosen swatch and only then goes on.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w390dp-h844dp-xxhdpi")
class WhoAddProfileTest {
    @get:Rule
    val compose = createComposeRule()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @After
    fun stop() = session.client.close()

    private fun creates() = sent.filter { it.getValue("op").jsonPrimitive.content == "createProfile" }

    private fun until(matcher: SemanticsMatcher) {
        val deadline = System.currentTimeMillis() + 30_000
        while (compose.onAllNodes(matcher).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "$matcher never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `Add profile opens the form in the sheet and makes one profile`() {
        val ana =
            ProfileInfo("p1", "Ana", "letter:ember", 0, hasPin = false, isOwner = true, syncing = false, kids = false)
        val ben = ana.copy(id = "p2", name = "Ben", avatar = "letter:amber", position = 1, isOwner = false)
        val snapshot = ProfilesSnapshot(listOf(ana, ben), active = "p1", locked = false, max = 5, kids = false)
        val json = CoreJson.encodeToString(ProfilesSnapshot.serializer(), snapshot)
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val subscribe = msg["op"]?.jsonPrimitive?.content == "subscribe"
            val result = if (subscribe) """{"sub":3,"snapshot":$json}""" else json
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        session.opened(running = true)
        gate.start()
        var added = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PickerScreen(gate, session, PickerMode.Start, onAdd = { added++ }, sheet = true, line = "Signed in.")
            }
        }
        until(hasContentDescription(FirstRunCopy.ADD_PROFILE))
        compose.onNodeWithContentDescription(FirstRunCopy.ADD_PROFILE).performClick()
        // The form, inside the sheet: its title, the field and Continue; Cancel goes back to the tiles.
        until(hasText(FirstRunCopy.CONTINUE))
        compose.onNodeWithText(SyncCopy.CANCEL).performClick()
        until(hasText(FirstRunCopy.WHO_TITLE))
        compose.onNodeWithContentDescription(FirstRunCopy.ADD_PROFILE).performClick()
        until(hasText(FirstRunCopy.CONTINUE))
        // An empty name is refused where it is typed, and nothing is made.
        compose.onNodeWithText(FirstRunCopy.CONTINUE).performClick()
        compose.waitForIdle()
        compose.onNodeWithText(ProfileCopy.problem("badName", "")).assertExists()
        assertTrue(creates().isEmpty())
        val field = SemanticsMatcher.keyIsDefined(SemanticsActions.SetText)
        compose.onNode(field).performTextInput("Kid")
        compose.onNodeWithContentDescription("Teal").performClick()
        compose.onNodeWithText(FirstRunCopy.CONTINUE).performClick()
        compose.waitUntil(10_000) { added == 1 }
        val made = creates().single()
        assertEquals("Kid", made.getValue("name").jsonPrimitive.content)
        assertEquals(ProfileCopy.formatAvatar("letter", "teal"), made.getValue("avatar").jsonPrimitive.content)
    }
}
