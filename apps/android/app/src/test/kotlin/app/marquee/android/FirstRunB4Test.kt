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
 * First run B4 (design/DECISION.md): the question steps as a film frame, 2.39:1 and no taller than 40 percent of the
 * screen, its question a heading over a 60 percent box, black with no still, centered on TV under the header with the
 * first choice focused. With MQ_SHOTS set, the render tests save theirs there.
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

    /** 2.39:1, no taller than 40 percent of the screen, and centered across it. */
    private fun framed() {
        val f = bounds(FILM_FRAME)
        val screen = root()
        assertEquals(FILM_RATIO, f.width / f.height, 0.02f, "the frame is $f")
        assertTrue(f.height <= screen.height * FILM_HEIGHT + 1f, "the frame $f is over 40 percent of $screen")
        assertEquals(f.left - screen.left, screen.right - f.right, 1.5f, "the frame $f is not centered in $screen")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the frame is 2_39 to 1 and spans the width`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        framed()
        assertEquals(root().width, bounds(FILM_FRAME).width, 1f)
    }

    @Test
    @Config(qualifiers = "w844dp-h390dp-xxhdpi")
    fun `phone on its side, the frame keeps under 40 percent of the height`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        framed()
        assertTrue(bounds(FILM_FRAME).width < root().width, "the frame took the whole width")
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the frame is 2_39 to 1, 40 percent of the height, centered`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        framed()
        assertEquals(1080f * FILM_HEIGHT, bounds(FILM_FRAME).height, 1f)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the frame and its bar sit in the middle of the space under the header`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        val header = bounds(STEP_HEADER)
        val frame = bounds(FILM_FRAME)
        val bar = bounds(FILM_BAR)
        // Under the header and over the safe area at the foot (60 on the 1080 canvas).
        val above = frame.top - header.bottom
        val below = root().bottom - 60f - bar.bottom
        assertTrue(abs(above - below) < 4f, "above $above, below $below")
        assertTrue(above > 40f, "the frame sits at the top: $above")
    }

    /** Each of [lines] shows whole: no line cut short or hidden. */
    private fun whole(vararg lines: String) {
        for (line in lines) {
            val node = compose.onNodeWithText(line, useUnmergedTree = true).fetchSemanticsNode()
            val layouts = mutableListOf<TextLayoutResult>()
            node.config[SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
            assertFalse(layouts.single().hasVisualOverflow, "cut: $line")
        }
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, both stream doors say their lines whole`() {
        question(Form.Tv, null)
        compose.waitForIdle()
        whole(StreamChoiceCopy.FAST.text, StreamChoiceCopy.BEST.text)
    }

    @Test
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
