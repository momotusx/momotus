package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.SubtitleSize
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.LOADING_TAG
import app.marquee.player.PhoneSubtitleSheet
import app.marquee.player.PlaybackController
import app.marquee.player.SEARCH_LANGUAGES
import app.marquee.player.SUBTITLE_SHEET_TAG
import app.marquee.player.TrackChoice
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone's subtitle sheet and loading screen (plans/player-polish.md 1.2 and 2.4): languages first, a level per
 * language, a full-height sheet over the landscape player; the loading screen alone with Back until the picture.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w844dp-h390dp")
class PhoneSubtitleSheetTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

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
    private val player = PlayingPlayer()
    private val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    /** The core's playback view, ready with a stream, as a scripted core answers its subscribe. */
    private fun readyView(snapshot: String = READY): PlaybackView {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$snapshot}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        return PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() }
    }

    @After
    fun stop() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private val rows =
        listOf(
            TrackChoice("off", "Off"),
            TrackChoice("late:en1", "CAKES, WEB 1080p", "OpenSubtitles v3", section = "English", language = "en"),
            TrackChoice(
                "late:en2",
                "FLUX, WEB-DL 1080p",
                "OpenSubtitles v3",
                section = "English",
                language = "en",
                fits = true,
            ),
            TrackChoice(
                "late:tr",
                "SubSense",
                section = "Turkish",
                language = "tr",
                nativeName = "Türkçe",
                selected = true,
            ),
            TrackChoice("0:0", "English", "In the file", section = "English", language = "en", inFile = true),
        )

    @Test
    fun `a language turns its best option on, its chevron opens its options, Languages comes back`() {
        val picks = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                var opened by remember { mutableStateOf<String?>(null) }
                PhoneSubtitleSheet(
                    rows,
                    opened,
                    { opened = it },
                    onPick = { picks += it },
                    onDone = {},
                    modifier = Modifier.width(420.dp).fillMaxHeight(),
                )
            }
        }
        compose.onNodeWithContentDescription("Turkish, Türkçe, 1 option").assertIsSelected()
        compose.onNodeWithContentDescription("English, 3 options").performClick()
        compose.runOnIdle { assertEquals(listOf("0:0"), picks, "the file's own English: it always fits the file") }
        compose.onNodeWithContentDescription("English options").performClick()
        // The options level: the file's track first, then the core's, the match named.
        compose.onNodeWithContentDescription("English, In the file").assertExists()
        compose.onNodeWithContentDescription("FLUX, WEB-DL 1080p, OpenSubtitles v3 · Same release group").performClick()
        compose.runOnIdle { assertEquals(listOf("0:0", "late:en2"), picks) }
        compose.onNodeWithContentDescription("Back to languages").performClick()
        compose.onNodeWithContentDescription("Off").assertExists()
        compose.onNodeWithContentDescription(SEARCH_LANGUAGES).performTextInput("tür")
        compose.waitForIdle()
        assertEquals(
            0,
            compose.onAllNodesWithContentDescription("English", substring = true).fetchSemanticsNodes().size,
        )
        compose.onNodeWithContentDescription("Turkish, Türkçe, 1 option").assertExists()
    }

    @Test
    fun `the system Back steps out of a language's options first, then closes the sheet, then leaves`() {
        var left = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, { left++ }) }
        }
        compose.runOnIdle { controller.attach(readyView(WITH_SUBTITLES)) }
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithContentDescription("Turkish options").performClick()
        compose.onNodeWithContentDescription("Back to languages").assertExists()
        compose.onNodeWithContentDescription("SubSense").assertExists()
        val back = { compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() } }
        back()
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Back to languages").assertDoesNotExist()
        compose.onNodeWithContentDescription("Turkish, Türkçe, 1 option").assertExists()
        assertEquals(0, left)
        back()
        compose.waitForIdle()
        compose.onNodeWithTag(SUBTITLE_SHEET_TAG).assertDoesNotExist()
        assertEquals(0, left)
        back()
        compose.runOnIdle { assertEquals(1, left, "then the player is left") }
    }

    @Test
    fun `a language's options end with the profile's subtitle size, stepped by Smaller and Larger`() {
        val sizes = mutableListOf<SubtitleSize>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneSubtitleSheet(
                    rows,
                    "en",
                    {},
                    onPick = {},
                    onDone = {},
                    modifier = Modifier.width(420.dp).fillMaxHeight(),
                    textSize = SubtitleSize.Large,
                    onSize = { sizes += it },
                )
            }
        }
        compose.onNodeWithText("Size").assertExists()
        compose.onNodeWithText("130%").assertExists()
        compose.onNodeWithText("Larger").performClick()
        compose.onNodeWithText("Smaller").performClick()
        compose.runOnIdle { assertEquals(listOf(SubtitleSize.ExtraLarge, SubtitleSize.Medium), sizes) }
    }

    @Test
    fun `Subtitles opens a full-height sheet at the right of the player, Back closes it`() {
        compose.setContent {
            MarqueeTheme(
                Form.Phone,
            ) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.onNodeWithContentDescription("Subtitles", substring = true).performClick()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val sheet = compose.onNodeWithTag(SUBTITLE_SHEET_TAG).fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        assertTrue(
            abs(sheet.height - (root.height - 24 * density)) < 2f,
            "full height but the 12 dp margins: $sheet in $root",
        )
        assertTrue(
            sheet.right > root.width * 0.9f && sheet.width <= 420 * density + 1f,
            "at the right, 420 dp at most: $sheet",
        )
        compose.activity.onBackPressedDispatcher.onBackPressed()
        compose.waitForIdle()
        compose.onNodeWithTag(SUBTITLE_SHEET_TAG).assertDoesNotExist()
    }

    @Test
    fun `while the first source buffers the loading screen stays alone, with no controls over it`() {
        compose.setContent {
            MarqueeTheme(
                Form.Phone,
            ) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { controller.attach(readyView()) }
        // The core answers on its own thread and the controller follows on the main looper: both are let run here.
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        assertEquals(0, compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().size)
        compose.onNodeWithContentDescription("Back").assertExists()
    }

    @Test
    fun `until the first frame the loading screen shows with Back alone, then the picture takes over`() {
        controller.showStatus("Starting 1080p, 1.5 GB from Torrentio")
        compose.setContent {
            MarqueeTheme(
                Form.Phone,
            ) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        compose.onNodeWithContentDescription("Back").assertExists()
        compose.onNodeWithContentDescription("Loading Sintel").assertExists()
        compose.onNodeWithText("Starting 1080p, 1.5 GB from Torrentio").assertExists()
        // No title bar words, no transport, no options: nothing to scrub before there is a picture.
        assertEquals(0, compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().size)
        compose.onNodeWithText("Sintel").assertDoesNotExist()
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
        compose.onNodeWithTag(LOADING_TAG).assertDoesNotExist()
        compose.onNodeWithText("Sintel").assertExists()
    }
}

/** A playback snapshot with its stream, for [readyView]. */
private const val READY =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[]}"""

/** A stream with an English track from OpenSubtitles and a Turkish one from SubSense, as the core lists them. */
private const val WITH_SUBTITLES =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[""" +
        """{"url":"http://127.0.0.1:7781/en.srt","lang":"eng","language":"en","languageName":"English",""" +
        """"nativeName":"English","name":"CAKES, WEB 1080p","source":"OpenSubtitles v3","forced":false,""" +
        """"hearingImpaired":false},""" +
        """{"url":"http://127.0.0.1:7781/tr.srt","lang":"tur","language":"tr","languageName":"Turkish",""" +
        """"nativeName":"Türkçe","name":"SubSense","source":"SubSense","forced":false,"hearingImpaired":false}]}"""
