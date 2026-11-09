package app.marquee.ui.phone

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.SubtitleBackground
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.SHIFT_TAG
import app.marquee.player.STRIP_TAG
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone's bar (plans/player-bar.md, L2): what left it, the quick shift, the strips, Next. Landscape at the
 * Pixel's 891 by 411 dp.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w891dp-h411dp")
class PhonePlayerBarTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private var play = BarPlay()
    private val controller get() = play.controller
    private val sizes = mutableListOf<SubtitleSize>()
    private val positions = mutableListOf<Int>()
    private val backgrounds = mutableListOf<SubtitleBackground>()
    private val nexts = mutableListOf<String>()

    @After
    fun stop() = play.close()

    /** Playing a series at one minute in, its controls up. */
    private fun playing(
        trakt: String = "off",
        /** Set after the core's snapshot, which would replace it. */
        setup: () -> Unit = {},
    ) {
        play.close()
        play = BarPlay(trakt)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhonePlayer(
                    controller,
                    false,
                    false,
                    {},
                    {},
                    { next, _ -> nexts += next.videoId },
                    {},
                    {},
                    onSubtitleSize = { sizes += it },
                    onSubtitlePosition = { positions += it },
                    onSubtitleBackground = { backgrounds += it },
                )
            }
        }
        play.attach { _, check -> compose.waitUntil(10_000) { check() } }
        compose.runOnIdle { play.player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        // Paused, so the controls stay up through the test.
        compose.runOnIdle {
            setup()
            controller.pause()
        }
        compose.waitForIdle()
    }

    private fun shows(description: String) =
        compose.onAllNodesWithContentDescription(description, substring = true).fetchSemanticsNodes().isNotEmpty()

    private fun back() = compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }

    private fun shift() = compose.onAllNodes(hasTestTag(SHIFT_TAG)).fetchSemanticsNodes().size

    @Test
    fun `no Trakt mark or pill, no volume and no More stay in the player`() {
        playing(trakt = "on")
        for (gone in listOf("Trakt", "Mute", "Volume")) assertTrue(!shows(gone), "$gone is still in the player")
        assertEquals(0, compose.onAllNodesWithText("Trakt", substring = true).fetchSemanticsNodes().size)
        assertTrue(!shows("More") || shows("More languages"), "a More button is on the bar")
    }

    @Test
    fun `the shift shows only while a subtitle that can be timed is on, steps 0_25 s, held faster, and resets`() {
        playing()
        assertEquals(0, shift(), "the shift showed with subtitles off")
        compose.runOnIdle { play.english() }
        compose.waitUntil(10_000) { shift() == 1 }
        compose.onNodeWithContentDescription("Subtitles later").performClick()
        compose.onNodeWithText("+0.25 s").assertExists()
        compose.onNodeWithContentDescription("Subtitles earlier").performClick()
        compose.onNodeWithContentDescription("Subtitles earlier").performClick()
        compose.onNodeWithText("−0.25 s").assertExists()
        // Held 2 s: ticks from 400 ms every 150 ms, growing each second, 4.5 s in all (D2).
        compose.mainClock.autoAdvance = false
        compose.onNodeWithContentDescription("Subtitles later").performTouchInput { down(center) }
        compose.mainClock.advanceTimeBy(2_000)
        compose.onNodeWithContentDescription("Subtitles later").performTouchInput { up() }
        compose.mainClock.advanceTimeBy(500)
        compose.mainClock.autoAdvance = true
        compose.onNodeWithText("+4.25 s").assertExists()
        compose.runOnIdle { assertEquals(4_250L, controller.subtitleDelayMs) }
        // A press on the value puts it back.
        compose
            .onNode(hasContentDescription("+4.25 s, subtitles later, press to reset"))
            .performClick()
        compose.onNodeWithText("0.0 s").assertExists()
        // Off: the shift goes.
        compose.runOnIdle { controller.selectText("off") }
        compose.waitUntil(5_000) { shift() == 0 }
    }

    @Test
    fun `Subtitles opens its strip, Back and its button close it, and the Style row closes first`() {
        playing()
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithText("More languages").assertExists()
        // Off, then the languages, as chips.
        compose.onNode(hasText("English")).assertExists()
        compose.onNodeWithText("Style").performClick()
        compose.onNodeWithText("Size").assertExists()
        compose.onNodeWithContentDescription("Larger").performClick()
        compose.onNodeWithContentDescription("Higher").performClick()
        compose.onNodeWithText("Outline").performClick()
        compose.runOnIdle {
            assertEquals(listOf(SubtitleSize.Large), sizes)
            assertEquals(listOf(12), positions)
            assertEquals(listOf(SubtitleBackground.Outline), backgrounds)
        }
        back()
        compose.waitForIdle()
        compose.onNodeWithText("Size").assertDoesNotExist()
        compose.onNodeWithText("More languages").assertExists()
        back()
        compose.waitForIdle()
        compose.onNodeWithText("More languages").assertDoesNotExist()
        compose.onNodeWithContentDescription("Back 10 seconds").assertExists()
        // The button again closes it too.
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithText("More languages").assertExists()
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithText("More languages").assertDoesNotExist()
    }

    @Test
    fun `a chip turns its language on, and More languages opens the full picker`() {
        playing()
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNode(hasText("English")).performClick()
        compose.waitUntil(5_000) { controller.subtitleOn }
        compose.waitUntil(5_000) { shift() == 2 }
        compose.onNodeWithText("More languages").performClick()
        compose.onNodeWithText("More languages").assertDoesNotExist()
        compose.onNodeWithContentDescription(app.marquee.player.SEARCH_LANGUAGES).assertExists()
    }

    @Test
    fun `a tap outside an open strip only closes it, the controls stay and nothing seeks`() {
        playing()
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithText("More languages").assertExists()
        compose.onRoot().performTouchInput { click(Offset(width * 0.25f, height * 0.3f)) }
        // Past the double tap's wait, when a single tap counts.
        compose.mainClock.advanceTimeBy(600)
        compose.waitForIdle()
        compose.onNodeWithText("More languages").assertDoesNotExist()
        compose.onNodeWithContentDescription("Back 10 seconds").assertExists()
        compose.runOnIdle { assertEquals(60_000L, play.player.currentPosition) }
        // Play, Back 10 and Forward 10 count as outside too: the first press only closes the strip (D21).
        for (press in listOf("Play", "Back 10 seconds", "Forward 10 seconds")) {
            compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
            compose.onNodeWithText("More languages").assertExists()
            compose.onNodeWithContentDescription(press).performClick()
            compose.onNodeWithText("More languages").assertDoesNotExist()
            compose.runOnIdle {
                assertEquals(60_000L, play.player.currentPosition, "$press seeked")
                assertEquals(PlaybackController.Stage.Paused, controller.stage, "$press played")
            }
        }
    }

    @Test
    fun `a strip opened while Skip shows sits over it, never on it`() {
        playing { controller.segments = listOf(Segment(SegmentKind.Intro, 0, 120_000, "TheIntroDB")) }
        compose.onNodeWithText("Skip intro").assertExists()
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.waitForIdle()
        val strip = compose.onNode(hasTestTag(STRIP_TAG)).fetchSemanticsNode().boundsInRoot
        val skip = compose.onNodeWithText("Skip intro").fetchSemanticsNode().boundsInRoot
        assertTrue(!strip.overlaps(skip), "the strip $strip meets Skip $skip")
    }

    @Test
    fun `Next shows only with a next episode, plays it, and Up Next coming closes an open strip`() {
        playing()
        assertTrue(!shows("Next episode"))
        compose.runOnIdle {
            controller.next = NextEpisode("tt1:1:2", "The Second", 1, 2)
            controller.durationMs = 600_000
        }
        compose.onNodeWithContentDescription("Next episode, S1:E2 The Second").performClick()
        compose.runOnIdle { assertEquals(listOf("tt1:1:2"), nexts) }
        compose.onNodeWithContentDescription("Subtitles, Off, 2 languages").performClick()
        compose.onNodeWithText("More languages").assertExists()
        compose.runOnIdle { controller.seekTo(590_000) }
        compose.waitUntil(5_000) {
            compose.onAllNodesWithText("More languages").fetchSemanticsNodes().isEmpty()
        }
    }
}
