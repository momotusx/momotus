package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.client.CoreJson
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

/** A search nothing matches repeats the query and offers the recommended addons, even when addons can search (audit P2). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SearchNoMatchTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())

    @After
    fun stop() = session.client.close()

    @Test
    fun `no match says the query and offers Recommended addons`() {
        session.client.attach { text ->
            val id =
                CoreJson
                    .parseToJsonElement(text)
                    .jsonObject["id"]
                    ?.jsonPrimitive
                    ?.longOrNull ?: return@attach
            session.client.onMessage(
                """{"id":$id,"ok":true,"result":{"sub":4,"snapshot":{"query":"zqxj","rows":[],"recent":[],"searchable":2}}}""",
            )
        }
        var addons = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) { SearchScreen(session, onOpen = {}, request = "zqxj", onAddons = { addons++ }) }
        }
        val line = "No matches for \"zqxj\" in your catalogs."
        val deadline = System.currentTimeMillis() + 60_000
        // The line is read out as it arrives (LiveSpoken), so its words sit in the unmerged tree.
        while (compose.onAllNodesWithText(line, useUnmergedTree = true).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the no-match line never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNodeWithText("Recommended addons").performClick()
        assertEquals(1, addons)
    }
}
