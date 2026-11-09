package app.marquee.ui.tv

import android.os.SystemClock
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performKeyPress
import androidx.compose.ui.test.pressKey
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.STRIP_TAG
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import android.view.KeyEvent as NativeKeyEvent

/**
 * The TV bar on a remote (plans/player-bar.md section 6): the focus row, the shift's adjust mode and held keys, Back's
 * order, and the way into an open strip. A series, English on (a subtitle that can be timed), paused so the controls
 * stay.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvPlayerBarKeysTest {
    @get:Rule
    val compose = createComposeRule()

    private var play = BarPlay()
    private val controller get() = play.controller

    @After
    fun stop() = play.close()

    private fun playing(
        focus: Boolean = true,
        /** Set after the core's snapshot, which would replace it. */
        setup: () -> Unit = {},
    ) {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, {}, { _, _ -> }, {}) } }
        play.attach { _, check -> compose.waitUntil(10_000) { check() } }
        compose.runOnIdle { play.player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        // After the core's snapshot, which names no next episode.
        compose.runOnIdle {
            setup()
            controller.next = NextEpisode("tt1:1:2", "The Second", 1, 2)
            play.english()
            controller.pause()
        }
        compose.waitUntil(10_000) { controller.quickShift != null }
        compose.waitForIdle()
        if (focus) focused().assert(hasContentDescription("Play"))
    }

    private fun focused(): SemanticsNodeInteraction = compose.onNode(isFocused())

    private fun SemanticsNodeInteraction.assert(matcher: androidx.compose.ui.test.SemanticsMatcher) =
        also { assertTrue(matcher.matches(fetchSemanticsNode()), "focus is on ${fetchSemanticsNode().config}") }

    private fun press(key: Key) = focused().performKeyInput { pressKey(key) }

    private val value =
        hasContentDescription("subtitles", substring = true) and
            hasContentDescription("press OK to adjust", substring = true)

    @Test
    fun `the focus row runs Audio, Speed, Versions, the transport, Subtitles, the shift's value, Episodes, Next`() {
        playing()
        // Audio shows once the file lists tracks; this fake lists none.
        press(Key.DirectionLeft)
        focused().assert(hasContentDescription("Back 10 seconds"))
        press(Key.DirectionLeft)
        focused().assert(hasContentDescription("Versions", substring = true))
        press(Key.DirectionLeft)
        focused().assert(hasContentDescription("Speed", substring = true))
        repeat(3) { press(Key.DirectionRight) }
        focused().assert(hasContentDescription("Play"))
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Forward 10 seconds"))
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Subtitles", substring = true))
        press(Key.DirectionRight)
        focused().assert(value)
        // Left and Right on the value move on, as from any control: never trapped without OK.
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Episodes"))
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Next episode, S1:E2 The Second"))
        press(Key.DirectionLeft)
        press(Key.DirectionLeft)
        focused().assert(value)
        press(Key.DirectionLeft)
        focused().assert(hasContentDescription("Subtitles", substring = true))
        // Up from the row goes to the position bar, Down from it to Play.
        press(Key.DirectionUp)
        focused().assert(hasContentDescription("Position"))
        press(Key.DirectionDown)
        focused().assert(hasContentDescription("Play"))
    }

    @Test
    @org.robolectric.annotation.GraphicsMode(org.robolectric.annotation.GraphicsMode.Mode.NATIVE)
    fun `the bar is one row, at most 140 points of the 1080 canvas, with no Trakt mark`() {
        playing(focus = false)
        val root =
            compose
                .onNode(
                    androidx.compose.ui.test
                        .isRoot(),
                ).fetchSemanticsNode()
                .boundsInRoot
        val bar =
            compose
                .onNode(
                    androidx.compose.ui.test
                        .hasTestTag(TV_BAR_TAG),
                ).fetchSemanticsNode()
                .boundsInRoot
        val scrub = compose.onNode(hasContentDescription("Position")).fetchSemanticsNode().boundsInRoot
        // The bar measured from the scrubber row's top to its foot, without the safe area under it.
        val play = compose.onNode(hasContentDescription("Play")).fetchSemanticsNode().boundsInRoot
        val points = (play.bottom - minOf(scrub.top, bar.top)) / root.height * 1080f
        assertTrue(points <= 140f, "the TV bar is $points points tall: $bar, $scrub, $play in $root")
        assertTrue(compose.onAllNodesWithText("Trakt", substring = true).fetchSemanticsNodes().isEmpty())
    }

    /** From Play to the shift's value. */
    private fun toValue() {
        repeat(3) { press(Key.DirectionRight) }
        focused().assert(value)
    }

    @Test
    fun `OK on the value adjusts, Left is earlier, Up leaves adjust mode and keeps the focus, Back leaves it first`() {
        playing()
        toValue()
        press(Key.DirectionCenter)
        press(Key.DirectionLeft)
        compose.onNodeWithText("−0.25 s").assertExists()
        focused().assert(value)
        press(Key.DirectionUp)
        focused().assert(value)
        // Out of adjust mode, Right moves the focus on.
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Episodes"))
        press(Key.DirectionLeft)
        press(Key.DirectionCenter)
        press(Key.DirectionRight)
        compose.onNodeWithText("0.0 s").assertExists()
        // Back leaves adjust mode only: the focus stays and the controls with it.
        press(Key.Back)
        focused().assert(value)
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Episodes"))
        compose.runOnIdle { assertEquals(0L, controller.subtitleDelayMs) }
        // OK leaves adjust mode too, and its release does not enter it again: Right then moves on.
        press(Key.DirectionLeft)
        press(Key.DirectionCenter)
        press(Key.DirectionCenter)
        press(Key.DirectionRight)
        focused().assert(hasContentDescription("Episodes"))
        compose.runOnIdle { assertEquals(0L, controller.subtitleDelayMs) }
    }

    /** Opens the subtitle strip by remote from Play: OK on Subtitles. */
    private fun openStrip() {
        repeat(2) { press(Key.DirectionRight) }
        press(Key.DirectionCenter)
        compose.waitUntil(5_000) { compose.onAllNodesWithText("More languages").fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    @Test
    fun `Reset in the strip puts the shift to 0 and leaves the focus on the value, never on nothing`() {
        playing()
        compose.runOnIdle { controller.setSubtitleDelay(500) }
        openStrip()
        var steps = 0
        while (!hasText("Reset").matches(focused().fetchSemanticsNode()) && steps++ < 10) press(Key.DirectionRight)
        press(Key.DirectionCenter)
        compose.waitForIdle()
        compose.runOnIdle { assertEquals(0L, controller.subtitleDelayMs) }
        compose.onNodeWithText("Reset").assertDoesNotExist()
        focused().assert(value)
    }

    @Test
    fun `Back closes the Style row first, the focus back on Style`() {
        playing()
        openStrip()
        compose.onNodeWithText("Style").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Size").fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        press(Key.Back)
        compose.waitForIdle()
        compose.onNodeWithText("Size").assertDoesNotExist()
        compose.onNodeWithText("More languages").assertExists()
        focused().assert(hasText("Style"))
    }

    @Test
    fun `a strip opened while Skip shows sits over it, never on it`() {
        playing(focus = false) {
            controller.segments = listOf(Segment(SegmentKind.Intro, 0, 120_000, "TheIntroDB"))
        }
        compose.onNodeWithText("Skip intro").assertExists()
        compose.onNode(hasContentDescription("Subtitles, ", substring = true)).performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("More languages").fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        val strip = compose.onNode(hasTestTag(STRIP_TAG)).fetchSemanticsNode().boundsInRoot
        val skip = compose.onNodeWithText("Skip intro").fetchSemanticsNode().boundsInRoot
        assertTrue(!strip.overlaps(skip), "the strip $strip meets Skip $skip")
    }

    @Test
    fun `long values never squeeze a pill, Next keeps its round target`() {
        play.close()
        play = BarPlay(english = "English (United Kingdom), for the hard of hearing, with the songs")
        playing(focus = false) {
            controller.versionLabel = "2160p WEB-DL HDR10 Dolby Vision Atmos 7.1 REMUX from the long release name"
        }
        val bar = compose.onNode(hasTestTag(TV_BAR_TAG)).fetchSemanticsNode().boundsInRoot
        val next = compose.onNode(hasContentDescription("Next episode", substring = true)).fetchSemanticsNode()
        val box = next.boundsInRoot
        assertTrue(box.width >= box.height - 1f, "Next is squeezed: $box")
        assertTrue(box.right <= bar.right + 1f, "Next runs past the bar: $box, $bar")
        val targets = compose.onAllNodes(hasClickAction()).fetchSemanticsNodes().map { it.boundsInRoot }
        for (t in targets.filter { it.top >= bar.top - 1f && it.bottom <= bar.bottom + 1f }) {
            assertTrue(t.width >= t.height - 1f, "a squeezed pill: $t")
        }
    }

    /**
     * Right held in adjust mode: a keydown at 0 ms, the remote's repeats every 50 ms, the keyup at [upAt]. The repeats
     * are ignored and the hold keeps its own pace (section 6).
     */
    private fun hold(upAt: Long) {
        compose.waitForIdle()
        val down = SystemClock.uptimeMillis()

        fun send(
            action: Int,
            at: Long,
            repeat: Int,
        ) = focused().performKeyPress(
            KeyEvent(NativeKeyEvent(down, down + at, action, NativeKeyEvent.KEYCODE_DPAD_RIGHT, repeat)),
        )
        compose.mainClock.autoAdvance = false
        send(NativeKeyEvent.ACTION_DOWN, 0, 0)
        var at = 0L
        var repeat = 0
        // Exact steps: the clock's own steps round up to whole frames.
        while (at + 50 < upAt) {
            compose.mainClock.advanceTimeBy(50, ignoreFrameDuration = true)
            at += 50
            send(NativeKeyEvent.ACTION_DOWN, at, ++repeat)
        }
        compose.mainClock.advanceTimeBy(upAt - at, ignoreFrameDuration = true)
        send(NativeKeyEvent.ACTION_UP, upAt, 0)
        compose.mainClock.advanceTimeBy(1_000)
        compose.mainClock.autoAdvance = true
    }

    @Test
    fun `a held key moves exactly 1_0 s when let go at 990 ms, and 0_25 s at 300 ms`() {
        playing()
        toValue()
        press(Key.DirectionCenter)
        hold(990)
        compose.runOnIdle { assertEquals(1_000L, controller.subtitleDelayMs) }
        hold(300)
        compose.runOnIdle { assertEquals(1_250L, controller.subtitleDelayMs) }
    }

    @Test
    fun `OK on Subtitles opens the strip on the checked chip, Down goes back, Up returns, Back closes it`() {
        playing()
        repeat(2) { press(Key.DirectionRight) }
        press(Key.DirectionCenter)
        compose.waitUntil(5_000) { compose.onAllNodesWithText("More languages").fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        focused().assert(hasText("English"))
        // Reset shows only while the value is not 0 (D22).
        assertTrue(compose.onAllNodesWithText("Reset").fetchSemanticsNodes().isEmpty())
        press(Key.DirectionDown)
        focused().assert(hasContentDescription("Subtitles", substring = true))
        compose.onNodeWithText("More languages").assertExists()
        press(Key.DirectionUp)
        focused().assert(hasText("English"))
        compose.runOnIdle { controller.setSubtitleDelay(500) }
        compose.onNodeWithText("Reset").assertExists()
        press(Key.Back)
        compose.waitForIdle()
        compose.onNodeWithText("More languages").assertDoesNotExist()
        focused().assert(hasContentDescription("Subtitles", substring = true))
        // From the scrubber Up reaches an open strip as well; from Play, not under it, Up goes to the scrubber.
        press(Key.DirectionCenter)
        compose.waitForIdle()
        press(Key.DirectionDown)
        repeat(2) { press(Key.DirectionLeft) }
        focused().assert(hasContentDescription("Play"))
        press(Key.DirectionUp)
        focused().assert(hasContentDescription("Position"))
        press(Key.DirectionUp)
        focused().assert(hasText("English"))
    }
}
