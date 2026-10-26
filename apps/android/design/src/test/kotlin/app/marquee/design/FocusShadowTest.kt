package app.marquee.design

import android.view.View
import androidx.collection.ScatterMap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.shadow.ShadowContext
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.LocalGraphicsContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertSame
import kotlin.test.assertTrue

/**
 * The TV focus shadow keeps one blurred bitmap a tile size, not one a frame. Compose caches a shadow's
 * bitmap per Shadow, alpha included, and drops the cache only when the app leaves the screen; below
 * API 26 those pixels sit in the Java heap, and a fading alpha filled the Fire TV's 192 MB with them
 * (OutOfMemoryError in DropShadowRenderer, 2026-10-24). On the 1920 canvas a poster's shadow is
 * (217 + 2 * 88) by (326 + 2 * 88) bytes, the crash's 197 KB.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp-mdpi")
class FocusShadowTest {
    @get:Rule
    val compose = createComposeRule()

    /**
     * The window's cached drop shadow renderers: Compose's AndroidShadowContext keeps them in a private map
     * (Compose 1.12.1; check the field's name when the BOM moves).
     */
    private fun cached(context: ShadowContext): List<Any?> {
        val field = context.javaClass.getDeclaredField("dropShadowCache").apply { isAccessible = true }
        val map = field.get(context) as ScatterMap<*, *>? ?: return emptyList()
        return buildList { map.forEachValue { add(it) } }
    }

    @Test
    fun `focus moving between two posters keeps one shadow bitmap`() {
        var on by mutableStateOf(0)
        lateinit var context: ShadowContext
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                context = LocalGraphicsContext.current.shadowContext
                // White, so the black shadow shows.
                Row(Modifier.background(Color.White).padding(PAD.dp)) {
                    repeat(2) { i ->
                        Row(
                            Modifier
                                .size(POSTER_W.dp, POSTER_H.dp)
                                .marqueeFocus(12.dp, Tokens.FOCUS_SCALE_ART, forced = on == i + 1, art = true),
                        ) {}
                    }
                }
            }
        }
        compose.mainClock.autoAdvance = false

        // Draws every frame of a focus fade, in and out.
        fun animate(frames: Int = FRAMES) {
            repeat(frames) {
                compose.mainClock.advanceTimeByFrame()
                compose.onRoot().captureToImage()
            }
        }

        // The brightness deep inside the first poster's shadow, which shows through the empty poster, below the
        // sheen; the scale and the rise barely move it there, so it follows the shadow's alpha.
        fun shade(): Float {
            val c = compose.onRoot().captureToImage().toPixelMap()[PAD + POSTER_W / 2, PAD + POSTER_H * 2 / 3]
            return (c.red + c.green + c.blue) / 3
        }
        on = 1
        // The first frame the shadow shows on, mid-fade.
        var fading = 1f
        repeat(FRAMES) {
            if (fading < 1f) return@repeat
            animate(1)
            fading = shade()
        }
        animate()
        val focused = shade()
        assertTrue(focused < fading && fading < 1f, "the shadow under a focus fade ($fading) and at rest ($focused)")
        val first = cached(context)
        assertEquals(1, first.size, "shadow renderers cached after the first poster's focus")
        on = 2
        animate()
        on = 1
        animate()
        val after = cached(context)
        assertEquals(1, after.size, "shadow renderers cached after focus moved to the other poster and back")
        assertSame(first.single(), after.single(), "the poster's shadow was built again")
    }

    /**
     * A chip is as wide as its words, so a shadow per chip would be a bitmap per name kept for the session: the
     * people linked on every title page (perf review r2, Q1). Chips cast none, so the cache stays empty.
     */
    @Test
    fun `chips of different widths add no shadow bitmap`() {
        val names = listOf("Denis Villeneuve", "Jon Spaihts", "Eric Roth", "Christopher Nolan", "Zendaya")
        var focused = 0
        lateinit var context: ShadowContext
        lateinit var view: View
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                context = LocalGraphicsContext.current.shadowContext
                view = LocalView.current
                Row(Modifier.background(Color.White).padding(PAD.dp)) {
                    names.forEach { name ->
                        Chip(name, selected = false, onClick = {}, onFocusChange = { if (it) focused++ })
                    }
                }
            }
        }
        // Native graphics start the window in touch mode, where a clickable takes no focus; a TV has no touch mode.
        compose.runOnUiThread { view.rootView.requestFocusFromTouch() }
        names.forEach { name ->
            compose.onNode(hasText(name) and hasClickAction()).requestFocus()
            compose.waitForIdle()
            compose.onRoot().captureToImage()
        }
        assertEquals(names.size, focused, "every chip took focus")
        assertEquals(0, cached(context).size, "shadow renderers cached after focus walked five names")
    }

    private companion object {
        const val PAD = 200
        const val POSTER_W = 217
        const val POSTER_H = 326

        // FOCUS_MS at 16 ms a frame, with room to settle.
        const val FRAMES = Tokens.FOCUS_MS / 16 + 4
    }
}
