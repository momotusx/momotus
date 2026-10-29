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
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
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
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * First run V1 Full Bleed (design/DECISION.md, design/exploration/onboarding-v2): the question steps as one still
 * filling the screen, black with no still. The phone keeps the question (a heading), its line and the doors at the
 * foot on a ramp, under the header; TV keeps them in a column at the lower left with the first choice focused. Every
 * word keeps 4.5:1 over a white still. With MQ_SHOTS set, the render tests save theirs there.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class FirstRunV1Test {
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

    /** The question's words, a heading. */
    private fun heading(text: String): Rect =
        compose
            .onNode(hasText(text) and SemanticsMatcher.keyIsDefined(SemanticsProperties.Heading))
            .fetchSemanticsNode()
            .boundsInRoot

    private fun door(text: String): Rect = compose.onNodeWithText(text).fetchSemanticsNode().boundsInRoot

    private fun px(dp: Int) = with(compose.density) { dp.dp.toPx() }

    /** Waits until the still has landed and faded in: the screen's right side, a third down, is no longer black. */
    private fun landed() {
        compose.waitUntil(5_000) {
            val f = bounds(FILM_FRAME)
            val pixels = compose.onRoot().captureToImage().toPixelMap()
            pixels[f.right.toInt() - 10, (f.top + f.height * 0.3f).toInt()] != Color.Black
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    /** The still's frame is the whole screen. */
    private fun fills() {
        val f = bounds(FILM_FRAME)
        val screen = root()
        val edges =
            listOf(f.left to screen.left, f.top to screen.top, f.right to screen.right, f.bottom to screen.bottom)
        for ((frame, edge) in edges) assertEquals(edge, frame, 1f, "the frame is $f in $screen")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the still fills the screen, the question under the header and the doors at its foot`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        fills()
        val q = heading("How would you like to begin?")
        assertTrue(q.top >= bounds(STEP_HEADER).bottom, "question $q, header ${bounds(STEP_HEADER)}")
        // No band under the last door: it ends within 48 dp of the window's foot, on screen.
        val last = door("I already use Momotus")
        val under = root().bottom - last.bottom
        assertTrue(under >= 0f && under <= px(48), "the last door $last ends $under px over the foot")
        assertEquals(last.bottom, bounds(FILM_BAR).bottom, 1f)
    }

    @Test
    @Config(qualifiers = "w844dp-h390dp-xxhdpi")
    fun `phone on its side, a block taller than the screen scrolls to its last door`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        fills()
        compose.onNodeWithText("I already use Momotus").performScrollTo()
        compose.waitForIdle()
        val last = door("I already use Momotus")
        assertTrue(last.top >= root().top && last.bottom <= root().bottom, "the last door $last is off ${root()}")
        // The still stays behind while the page scrolls.
        fills()
    }

    /** The block sits at the lower left: left of 62 percent of the width, on the safe area at the foot. */
    private fun lowerLeft() {
        val bar = bounds(FILM_BAR)
        val screen = root()
        val scale = screen.width / 1920f
        assertTrue(bar.right < screen.left + screen.width * 0.62f, "the block $bar reaches past 62 percent of $screen")
        assertEquals(screen.bottom - 60f * scale, bar.bottom, 4f, "the block $bar is not at the foot of $screen")
        assertEquals(80f * scale, bar.left - screen.left, 2f, "the block $bar is off the safe area")
        assertTrue(heading("How would you like to begin?").top >= bounds(STEP_HEADER).bottom)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the still fills the screen and the block sits at the lower left`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        fills()
        lowerLeft()
        // The doors are stacked, the block's width each.
        val first = door("I use Stremio")
        val last = door("I already use Momotus")
        assertTrue(last.top > first.bottom, "first $first, last $last")
        assertEquals(first.width, last.width, 1f)
    }

    @Test
    @Config(qualifiers = "w720dp-h540dp-xhdpi")
    fun `TV at 4 to 3, the block sits at the lower left`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        fills()
        lowerLeft()
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
    fun `the question is a real heading over the still, and the step is counted with Back from step 2`() {
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
    fun `phone, a step change is announced by its question, the step's pane title`() {
        question(Form.Phone, null)
        compose.waitForIdle()
        compose
            .onNode(
                SemanticsMatcher.expectValue(SemanticsProperties.PaneTitle, StreamChoiceCopy.QUESTION_TITLE),
            ).assertExists()
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

    /** With no still the screen is black where no words or doors are: at the right side, top and middle. */
    private fun black() {
        val f = bounds(FILM_FRAME)
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        assertEquals(Color.Black, pixels[f.right.toInt() - 4, f.top.toInt() + 4])
        assertEquals(Color.Black, pixels[f.right.toInt() - 4, f.center.y.toInt()])
        compose
            .onNode(
                hasText("How would you like to begin?") and hasAnyAncestor(hasTestTag(FILM_FRAME)),
            ).assertExists()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, no still, the screen stays black with its question`() {
        doors(Form.Tv, null)
        compose.waitForIdle()
        black()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, no still, the screen stays black with its question`() {
        doors(Form.Phone, null)
        compose.waitForIdle()
        black()
    }

    /** WCAG contrast of white on [back]. */
    private fun whiteOn(back: Color): Float = (1.05f) / (back.luminance() + 0.05f)

    /** A promo collage as a still: two dark panels on a cream ground, 40 px of it at the sides, 60 above and below. */
    private fun collage(): Bitmap {
        val b = Bitmap.createBitmap(480, 270, Bitmap.Config.ARGB_8888)
        b.eraseColor(0xFFFCF1E6.toInt())
        val c = Canvas(b)
        c.drawRect(40f, 60f, 230f, 210f, Paint().apply { color = 0xFF3A4A3E.toInt() })
        c.drawRect(250f, 60f, 440f, 210f, Paint().apply { color = 0xFF5A3E2E.toInt() })
        return b
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a still's flat margin is cropped, its gutters kept`() {
        val t = trimFlatEdges(collage())
        assertEquals(400 to 150, t.width to t.height)
        assertEquals(0xFF3A4A3E.toInt(), t.getPixel(0, 0))
        assertEquals(0xFF5A3E2E.toInt(), t.getPixel(399, 149))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a still with no flat margin stays whole`() {
        val flat = white()
        assertTrue(trimFlatEdges(flat) === flat)
        val scene = scene()
        assertTrue(trimFlatEdges(scene) === scene)
    }

    /**
     * Over the white still: just left of the question's words, at their middle, the wash or the ramp; and inside the
     * header's Browse first, before its words, the wash under the header.
     */
    private fun wordsKeepContrast() {
        val q = heading("How would you like to begin?")
        val link = door("Browse first")
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        val back = pixels[q.left.toInt() - 4, q.center.y.toInt()]
        assertTrue(whiteOn(back) >= 4.5f, "the question reads ${whiteOn(back)}: $back")
        val head = pixels[link.left.toInt() + 2, link.center.y.toInt()]
        assertTrue(whiteOn(head) >= 4.5f, "Browse first reads ${whiteOn(head)}: $head")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, over a white still the question and the header keep 4_5 to 1`() {
        serve(white())
        doors(Form.Tv, still)
        landed()
        val f = bounds(FILM_FRAME)
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        // The still landed: the right side, clear of the wash, is white.
        assertEquals(Color.White, pixels[f.right.toInt() - 10, f.center.y.toInt()])
        wordsKeepContrast()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, over a white still the question and the header keep 4_5 to 1, the still showing above the ramp`() {
        serve(white())
        doors(Form.Phone, still)
        landed()
        // Where the header's wash (220 dp) has gone, or where the ramp (170 dp over the question) starts if lower,
        // the still shows nearly as it is.
        val y = maxOf(px(220), bounds(FILM_BAR).top - px(170))
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        val shows = pixels[root().center.x.toInt(), y.toInt()]
        assertTrue(shows.luminance() > 0.6f, "the still is hidden at $y: $shows")
        wordsKeepContrast()
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
