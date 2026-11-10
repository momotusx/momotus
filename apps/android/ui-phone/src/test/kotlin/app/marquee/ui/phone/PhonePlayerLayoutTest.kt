package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.media3.common.VideoSize
import androidx.media3.common.text.Cue
import androidx.media3.ui.SubtitleView
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.SubtitleSize
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.CUE_FLOOR
import app.marquee.player.CUE_LINE
import app.marquee.player.CUE_LINES
import app.marquee.player.CUE_RISE
import app.marquee.player.PlaybackController
import app.marquee.player.STRIP_TAG
import app.marquee.player.SUBTITLES_TAG
import app.marquee.player.subtitleScale
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone player's layout in landscape (the owner's rule, as on the iPhone and the web): the transport in the
 * middle of the screen, the bar and the options row (one group, never spread edge to edge) at the foot, and the
 * subtitles 8 dp over the bar while the controls show, smaller (down to 75%) when they would meet the transport.
 * 540 dp tall: Robolectric's fonts set the controls taller than a phone's (178 dp against about 124 on the Pixel).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class PhonePlayerLayoutTest {
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

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    private fun playing(size: SubtitleSize = SubtitleSize.Medium) {
        controller.subtitleSize = size
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}) }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
    }

    private fun bounds(description: String) =
        compose.onNodeWithContentDescription(description).fetchSemanticsNode().boundsInRoot

    private fun controlsHidden() {
        compose.waitUntil(10_000) {
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().isEmpty()
        }
        compose.waitForIdle()
    }

    /**
     * Two lines of cues as the subtitle view will draw them: its first cue's foot (from the line the view was given,
     * as Media3's painter places it), the view's text size, and the span the cue's lines keep to (with the view's
     * own padding beside the text), read from the view.
     */
    private class Cues(
        val top: Float,
        val foot: Float,
        val text: Float,
        val left: Float,
        val right: Float,
    )

    private fun cues(): Cues {
        fun find(view: View): SubtitleView? =
            view as? SubtitleView ?: (view as? ViewGroup)?.let { group ->
                (0 until group.childCount).firstNotNullOfOrNull { find(group.getChildAt(it)) }
            }
        val view = requireNotNull(find(compose.activity.window.decorView)) { "no subtitle view" }

        fun field(name: String) =
            SubtitleView::class.java
                .getDeclaredField(name)
                .apply { isAccessible = true }
                .getFloat(view)
        val height = view.height.toFloat()
        val width = view.width.toFloat()
        val text = field("defaultTextSize")

        @Suppress("UNCHECKED_CAST")
        val cue =
            (
                SubtitleView::class.java
                    .getDeclaredField("cues")
                    .apply { isAccessible = true }
                    .get(view) as List<Cue>
            ).firstOrNull()
        val foot =
            when {
                cue == null -> error("no cue on screen")
                cue.lineType == Cue.LINE_TYPE_FRACTION && cue.lineAnchor == Cue.ANCHOR_TYPE_END -> cue.line * height
                cue.line == Cue.DIMEN_UNSET -> height * (1 - field("bottomPaddingFraction"))
                cue.lineType == Cue.LINE_TYPE_NUMBER && cue.line == -1f -> height
                else -> error("a cue on a line of its own: $cue")
            }
        val side = text * height * TEXT_PADDING
        val (left, right) =
            if (cue.size == Cue.DIMEN_UNSET || cue.size >= 1f) {
                0f to width
            } else {
                width * (cue.position - cue.size / 2) - side to width * (cue.position + cue.size / 2) + side
            }
        return Cues(foot - CUE_LINES * CUE_LINE * text * height, foot, text, left, right)
    }

    /** The cues' size with the controls hidden: the system caption size (1 here) times the profile's. */
    private fun full(size: SubtitleSize) = SubtitleView.DEFAULT_TEXT_SIZE_FRACTION * subtitleScale(size)

    @Test
    fun `the transport in the middle, one slim bar at the foot with the scrubber between the pill groups`() {
        playing()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val transport = bounds("Back 10 seconds")
        val bar = compose.onNodeWithTag(BAR_TAG).fetchSemanticsNode().boundsInRoot
        val scrub = bounds("Position")
        assertTrue(transport.bottom <= bar.top, "the transport sits over the bar: $transport, $bar, $root")
        // In the middle of the screen, as on the iPhone and the web; the bar at the foot (plans/player-bar.md).
        val play = bounds("Pause")
        assertTrue(abs(play.center.y - root.center.y) < 2f, "the transport is in the middle: ${play.center.y}")
        assertTrue(abs(play.center.x - root.center.x) < 2f, "and centred across: ${play.center.x}")
        assertTrue(root.bottom - bar.bottom <= 12.5f, "the bar keeps to the foot: ${bar.bottom}")
        assertTrue(bar.height <= 72f, "one slim row: ${bar.height}")
        // Speed and Versions left of the scrubber, Subtitles right of it, all in the one row.
        val speed = compose.onNodeWithContentDescription("Speed", substring = true).fetchSemanticsNode().boundsInRoot
        val subtitles =
            compose.onNodeWithContentDescription("Subtitles", substring = true).fetchSemanticsNode().boundsInRoot
        assertTrue(speed.right <= scrub.left && subtitles.left >= scrub.right, "$speed, $scrub, $subtitles")
        assertTrue(abs(speed.center.y - scrub.center.y) < 2f && abs(subtitles.center.y - scrub.center.y) < 2f)
    }

    @Test
    fun `the subtitles sit just above the controls while they show, at the foot when they hide`() {
        playing()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val area = compose.onNodeWithTag(SUBTITLES_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertEquals(root, area, "the subtitles keep the whole picture area, so they keep their size")
        player.say("Who are you, and what are you doing here")
        compose.waitForIdle()
        val shown = cues()
        val bar = bounds("Position")
        assertTrue(shown.foot <= bar.top - 8f + 0.5f, "8 dp over the bar: ${shown.foot} against ${bar.top}")
        assertTrue(shown.foot >= bar.top - 8f - 0.5f, "just over it, not higher: ${shown.foot}")
        assertEquals(full(SubtitleSize.Medium), shown.text, "one line fits under the transport: full size")
        assertEquals(0f to root.width, shown.left to shown.right, "nothing in the corner: the whole width")
        // A cue with no line of its own (as SubRip has it) is lifted the same way.
        player.say("Who are you, and what are you doing here", line = Cue.DIMEN_UNSET)
        compose.waitForIdle()
        assertTrue(cues().foot <= bar.top - 8f + 0.5f, "with no line, over the bar too: ${cues().foot}")
        // The controls hide while it plays: the subtitles go back to the foot, at full size.
        player.say("Who are you, and what are you doing here")
        controlsHidden()
        val hidden = cues()
        assertEquals(
            root.height * (1 - SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION),
            hidden.foot,
            0.5f,
            "at the foot again, 8% over the screen's, not on its last pixel",
        )
        assertEquals(full(SubtitleSize.Medium), hidden.text, "full size")
    }

    @Test
    fun `under a wide film the hidden controls leave the subtitles 8% of the picture over the picture's foot`() {
        // A 2.39:1 film (1920 by 803): PhonePlayer passes the controller's ratio, so the foot is the picture's.
        player.pictureSize = VideoSize(1920, 803)
        playing()
        player.say("Who are you, and what are you doing here")
        controlsHidden()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val picture = minOf(root.height, root.width * 803f / 1920f)
        val foot = cues().foot
        assertEquals((root.height + picture) / 2 - picture * CUE_RISE, foot, 0.5f, "8% of the picture over its foot")
    }

    @Test
    fun `four lines at the largest size shrink under the transport, down to 75%, while the controls show`() {
        playing(SubtitleSize.ExtraLarge)
        player.say(
            "Who are you, and what are you doing here\nwith a sword and a dragon's tooth\n" +
                "in a land where no one goes\nand nothing grows",
        )
        compose.waitForIdle()
        val shown = cues()
        val bar = bounds("Position")
        assertTrue(shown.foot <= bar.top - 8f + 0.5f, "8 dp over the bar: ${shown.foot} against ${bar.top}")
        // Between the slim bar and the transport in the middle of a 540 dp screen four lines have room for neither
        // 100% nor 85%: the cues draw at the floor (two lines fit whole over the slim bar).
        assertEquals(full(SubtitleSize.ExtraLarge) * CUE_FLOOR, shown.text, 1e-6f, "at 75%")
        controlsHidden()
        assertEquals(full(SubtitleSize.ExtraLarge), cues().text, "back to full size once the controls hide")
    }

    @Test
    fun `beside Skip the lines keep clear of it, centred, with the controls up or hidden`() {
        controller.segments = listOf(Segment(SegmentKind.Intro, 0, 120_000, "TheIntroDB"))
        playing()
        player.say("Who are you, and what are you doing here")
        compose.waitForIdle()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot

        fun skip() =
            compose
                .onNodeWithText("Skip intro")
                .fetchSemanticsNode()
                .boundsInRoot.left
        val shown = cues()
        assertTrue(shown.right <= skip(), "clear of Skip: the lines end at ${shown.right}, Skip starts at ${skip()}")
        assertTrue(abs((shown.left + shown.right) / 2 - root.center.x) < 1f, "centred: ${shown.left} to ${shown.right}")
        // One line, still one in 78% of the width: full size.
        assertEquals(full(SubtitleSize.Medium), shown.text, "at full size")
        controlsHidden()
        val hidden = cues()
        assertTrue(hidden.right <= skip(), "clear of Skip at the foot: ${hidden.right} against ${skip()}")
    }

    /**
     * Pixel walk 2026-11-09 L23: with the strip and its Style row open the lift put the lines on the transport, so
     * they could not be read while the viewer tuned them. They go over the transport instead, under the title bar,
     * and stay clear of the strip.
     */
    @Test
    fun `with the strip and Style open the lines go over the transport, never under it`() {
        playing()
        compose.onNode(hasContentDescription("Subtitles", substring = true) and hasClickAction()).performClick()
        compose.onNodeWithText("Style").performClick()
        compose.onNodeWithText("Size").assertExists()
        player.say("You're a fool for traveling alone\nso completely unprepared")
        compose.waitForIdle()
        val shown = cues()
        val back = bounds("Back 10 seconds")
        val play = bounds("Pause")
        val transport = minOf(back.top, play.top) to maxOf(back.bottom, play.bottom)
        val strip = compose.onNodeWithTag(STRIP_TAG).fetchSemanticsNode().boundsInRoot
        assertTrue(
            shown.foot <= transport.first || shown.top >= transport.second,
            "the lines ${shown.top} to ${shown.foot} meet the transport ${transport.first} to ${transport.second}",
        )
        assertTrue(shown.foot <= strip.top - 4f, "the lines' foot ${shown.foot} reaches the strip at ${strip.top}")
    }

    private companion object {
        /** The subtitle view's room beside a cue's text, of its text size (SubtitlePainter's inner padding). */
        const val TEXT_PADDING = 0.125f
    }
}
