package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.text.TextLayoutResult
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.FirstRunStill
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.Uri
import coil3.asImage
import coil3.decode.DataSource
import coil3.fetch.FetchResult
import coil3.fetch.Fetcher
import coil3.fetch.ImageFetchResult
import coil3.request.Options
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * First run B4 (design/DECISION.md): the question steps as a 2.39:1 film frame as wide as the screen while the height
 * the header and the bar leave allows, its question a heading over a 60 percent box, black with no still, the bar on
 * the header's edge, centered on TV with the first choice focused. With MQ_SHOTS set, the render tests save theirs there.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class FirstRunB4Test {
    @get:Rule
    val compose = createComposeRule()

    @After
    fun reset() = SingletonImageLoader.reset()

    private val still = FirstRunStill(background = "https://art.example/still.jpg", name = "Sintel")

    /** Every picture is [picture]. */
    private fun serve(picture: Bitmap) {
        val image = picture.asImage()
        val factory =
            Fetcher.Factory<Uri> { _, _: Options, _: ImageLoader ->
                Fetcher { ImageFetchResult(image, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader
                .Builder(RuntimeEnvironment.getApplication())
                .components {
                    add(factory)
                }.build(),
        )
    }

    private fun white() =
        Bitmap.createBitmap(48, 20, Bitmap.Config.ARGB_8888).apply {
            eraseColor(android.graphics.Color.WHITE)
        }

    private fun scene(): Bitmap {
        val b = Bitmap.createBitmap(956, 400, Bitmap.Config.ARGB_8888)
        val c = Canvas(b)
        c.drawRect(
            0f,
            0f,
            956f,
            400f,
            Paint().apply {
                shader =
                    LinearGradient(0f, 0f, 0f, 400f, 0xFFD9B98A.toInt(), 0xFF6E4A3A.toInt(), Shader.TileMode.CLAMP)
            },
        )
        c.drawRect(120f, 140f, 360f, 400f, Paint().apply { color = 0xFFE3B23C.toInt() })
        c.drawRect(620f, 60f, 956f, 400f, Paint().apply { color = 0xFFC98C8C.toInt() })
        return b
    }

    private fun doors(
        form: Form,
        still: FirstRunStill?,
    ) = compose.setContent {
        MarqueeTheme(form) { OnboardingScreen(onDefaults = {}, onImport = {}, onPair = {}, still = still) }
    }

    private fun question(
        form: Form,
        still: FirstRunStill?,
    ) = compose.setContent {
        MarqueeTheme(form) { StreamQuestionPage(still, null, onBack = {}, onSkip = {}, onAnswer = {}) }
    }

    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot

    private fun root(): Rect = compose.onRoot().fetchSemanticsNode().boundsInRoot

    /** Waits until the still has landed and faded in: the frame's top left is no longer black. */
    private fun landed() {
        compose.waitUntil(5_000) {
            val f = bounds(FILM_FRAME)
            compose.onRoot().captureToImage().toPixelMap()[f.left.toInt() + 10, f.top.toInt() + 10] != Color.Black
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    /** 2.39:1 and centered across the screen. */
    private fun framed() {
        val f = bounds(FILM_FRAME)
        val screen = root()
        assertEquals(FILM_RATIO, f.width / f.height, 0.02f, "the frame is $f")
        assertEquals(f.left - screen.left, screen.right - f.right, 1.5f, "the frame $f is not centered in $screen")
    }

    /** The header and the bar under the frame keep one column edge. */
    private fun oneEdge() {
        val header = bounds(STEP_HEADER)
        val bar = bounds(FILM_BAR)
        assertEquals(header.left, bar.left, 1f, "header $header, bar $bar")
        assertEquals(header.right, bar.right, 1f, "header $header, bar $bar")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the frame is 2_39 to 1 and as wide as the screen, the bar on the header's edge`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        framed()
        assertEquals(root().width, bounds(FILM_FRAME).width, 1f)
        oneEdge()
    }

    @Test
    @Config(qualifiers = "w844dp-h390dp-xxhdpi")
    fun `phone on its side, a bar taller than the rest leaves the frame its least height`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        framed()
        assertEquals(root().height * FILM_FLOOR, bounds(FILM_FRAME).height, 2f)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the frame takes the height the header and the bar leave, the bar on the header's edge`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        framed()
        // 24 above the frame and under the bar, over the 60 safe area at the foot of the 1080 canvas.
        assertEquals(24f, bounds(FILM_FRAME).top - bounds(STEP_HEADER).bottom, 1f)
        assertEquals(24f, root().bottom - 60f - bounds(FILM_BAR).bottom, 1f)
        assertTrue(bounds(FILM_FRAME).height > 1080f * 0.4f, "the frame is small: ${bounds(FILM_FRAME)}")
        oneEdge()
        // The doors share the bar from edge to edge.
        val bar = bounds(FILM_BAR)
        val first = compose.onNodeWithText("I use Stremio").fetchSemanticsNode().boundsInRoot
        val last = compose.onNodeWithText("I already use Momotus").fetchSemanticsNode().boundsInRoot
        assertEquals(bar.left, first.left, 1f)
        assertEquals(bar.right, last.right, 1f)
    }

    @Test
    @Config(qualifiers = "w720dp-h540dp-xhdpi")
    fun `TV at 4 to 3, the full-width frame and its bar sit in the middle under the header`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        val header = bounds(STEP_HEADER)
        val frame = bounds(FILM_FRAME)
        val bar = bounds(FILM_BAR)
        // A 4:3 TV is 1920 by 1440 on the canvas: the width binds and the height left over is shared.
        val scale = root().width / 1920f
        assertEquals(root().width, frame.width, 1f)
        // Under the header and over the safe area at the foot (60 on the canvas).
        val above = frame.top - header.bottom
        val below = root().bottom - 60f * scale - bar.bottom
        assertTrue(above > 24f * scale + 20f, "nothing was left over: $above")
        assertTrue(abs(above - below) < 4f, "above $above, below $below")
    }

    /**
     * Each of [lines] shows whole, no line cut short or hidden (native graphics: only they measure text as a
     * device does).
     */
    private fun whole(vararg lines: String) {
        for (line in lines) {
            val node = compose.onNodeWithText(line, useUnmergedTree = true).fetchSemanticsNode()
            val layouts = mutableListOf<TextLayoutResult>()
            node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
            assertFalse(layouts.single().hasVisualOverflow, "cut: $line")
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, both stream doors say their lines whole`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        whole(StreamChoiceCopy.FAST.text, StreamChoiceCopy.BEST.text)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the three doors say their lines whole`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        whole("Bring your addons, library and Continue Watching over with a code.")
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the first choice takes the focus on open`() {
        doors(Form.Tv, null)
        compose.waitUntil(
            5_000,
        ) { runCatching { compose.onNode(isFocused()).assert(hasText("I use Stremio")) }.isSuccess }
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `the question is a real heading inside the frame, and the step is counted with Back from step 2`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        val heading = SemanticsMatcher.keyIsDefined(SemanticsProperties.Heading)
        compose
            .onNode(
                hasText(StreamChoiceCopy.QUESTION_TITLE) and heading and
                    hasAnyAncestor(hasTestTag(FILM_FRAME)),
            ).assertExists()
        compose.onNodeWithText("Step 3 of 4").assertExists()
        compose.onNodeWithText("Back").assertExists()
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the step count is read, not shown, and the first step has no Back`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Step 1 of 4").assertExists()
        compose.onNodeWithText("Step 1 of 4").assertDoesNotExist()
        compose.onNodeWithText("Back").assertDoesNotExist()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `no still, the frame stays black with its question`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        val f = bounds(FILM_FRAME)
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        assertEquals(Color.Black, pixels[f.left.toInt() + 20, f.top.toInt() + 20])
        assertEquals(Color.Black, pixels[f.right.toInt() - 20, f.top.toInt() + 20])
        compose
            .onNode(
                hasText("How would you like to begin?") and hasAnyAncestor(hasTestTag(FILM_FRAME)),
            ).assertExists()
    }

    /** WCAG contrast of white on [back]. */
    private fun whiteOn(back: Color): Float = (1.05f) / (back.luminance() + 0.05f)

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `over a white still the question keeps 4_5 to 1 on its box`() {
        serve(white())
        doors(Form.Tv, still)
        landed()
        val f = bounds(FILM_FRAME)
        val q =
            compose
                .onNode(hasText("How would you like to begin?") and hasAnyAncestor(hasTestTag(FILM_FRAME)))
                .fetchSemanticsNode()
                .boundsInRoot
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        // The still landed: the frame's corner is white.
        assertEquals(Color.White, pixels[f.left.toInt() + 10, f.top.toInt() + 10])
        // Just left of the words, inside the box's padding: the scrim over white.
        val box = pixels[q.left.toInt() - 4, q.center.y.toInt()]
        assertTrue(whiteOn(box) >= 4.5f, "the box reads ${whiteOn(box)}: $box")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone renders`() {
        serve(scene())
        doors(Form.Phone, still)
        landed()
        shot("welcome-phone")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone render of the stream question`() {
        serve(scene())
        question(Form.Phone, still)
        landed()
        shot("choice-phone")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV render of the doors`() {
        serve(scene())
        doors(Form.Tv, still)
        landed()
        shot("welcome-tv")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV render of the stream question with no still`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        shot("choice-tv-no-still")
    }
}
