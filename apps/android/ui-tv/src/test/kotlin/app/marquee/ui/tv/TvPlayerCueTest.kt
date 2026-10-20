package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onRoot
import androidx.media3.common.text.Cue
import androidx.media3.ui.SubtitleView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
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
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The TV player's cues while the controls show: their foot 8 dp over the controls band, as on the phone (the Fire TV walk). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvPlayerCueTest {
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

    /** The first cue's foot, in px from the top, as the subtitle view will draw it. */
    private fun cueFoot(): Float {
        fun find(view: View): SubtitleView? =
            view as? SubtitleView ?: (view as? ViewGroup)?.let { group ->
                (0 until group.childCount).firstNotNullOfOrNull { find(group.getChildAt(it)) }
            }
        val view = requireNotNull(find(compose.activity.window.decorView)) { "no subtitle view" }

        // Media3 keeps the cues in a private field (as the phone's layout test reads them too).
        val field =
            requireNotNull(runCatching { SubtitleView::class.java.getDeclaredField("cues") }.getOrNull()) {
                "SubtitleView has no private cues field any more (a Media3 upgrade?): read the cues another way"
            }

        @Suppress("UNCHECKED_CAST")
        val cue = (field.apply { isAccessible = true }.get(view) as List<Cue>).first()
        val height = view.height.toFloat()
        return when {
            cue.lineType == Cue.LINE_TYPE_FRACTION && cue.lineAnchor == Cue.ANCHOR_TYPE_END -> cue.line * height
            cue.lineType == Cue.LINE_TYPE_NUMBER && cue.line == -1f -> height
            else -> error("a cue on a line of its own: $cue")
        }
    }

    @Test
    fun `while the controls show the cue sits over their band, and goes back to the foot when they hide`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        player.say("Who are you, and what are you doing here")
        compose.waitForIdle()
        val bar = compose.onNodeWithContentDescription("Position").fetchSemanticsNode().boundsInRoot
        val transport = compose.onNodeWithContentDescription("Back 10 seconds").fetchSemanticsNode().boundsInRoot
        val band = minOf(bar.top, transport.top)
        assertTrue(cueFoot() <= band, "the cue sits under the controls: ${cueFoot()} against $band")
        // The controls hide after four seconds of real time.
        compose.waitUntil(10_000) {
            compose.onAllNodesWithContentDescription("Back 10 seconds").fetchSemanticsNodes().isEmpty()
        }
        compose.waitForIdle()
        assertEquals(
            compose
                .onRoot()
                .fetchSemanticsNode()
                .boundsInRoot.bottom,
            cueFoot(),
            "at the foot again",
        )
    }
}
