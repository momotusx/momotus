package app.marquee.design

import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import coil3.Image
import coil3.intercept.Interceptor
import coil3.request.CachePolicy
import coil3.request.ErrorResult
import coil3.request.ImageRequest
import coil3.request.ImageResult
import coil3.size.Size
import kotlinx.coroutines.runBlocking
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Incognito in the design layer (plans/batch-10-incognito.md 5.2, 5.5): the mark, the keyboard, images, trailers. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class IncognitoDesignTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the mark is the glyph on a sea circle`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                Box(Modifier.size(100.dp).testTag("mark")) { AvatarArt(INCOGNITO_GLYPH, null, "I", 100.dp) }
            }
        }
        val pixels = compose.onNodeWithTag("mark").captureToImage().toPixelMap()
        // Inside the circle, outside the glyph (60 percent of it, centred).
        assertEquals(Color(0xFF1A78A0), pixels[50, 6])
        // Some of the glyph is white.
        assertTrue((20 until 80).any { x -> (20 until 80).any { y -> pixels[x, y] == Color.White } })
    }

    private lateinit var view: View

    private fun field(incognito: Boolean): Int {
        compose.setContent {
            view = LocalView.current
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalIncognito provides incognito) {
                    var text by remember { mutableStateOf("") }
                    MqTextField(text, { text = it }, label = "Search", placeholder = "Titles")
                }
            }
        }
        compose.onNodeWithContentDescription("Search").performClick()
        compose.waitForIdle()
        val info = EditorInfo()
        compose.runOnIdle { checkNotNull(view.onCreateInputConnection(info)) { "no editor opened" } }
        return info.imeOptions
    }

    @Test
    fun `in Incognito the editor asks the keyboard to learn nothing`() {
        assertTrue(field(incognito = true) and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0)
    }

    @Test
    fun `outside Incognito the keyboard may learn`() {
        assertFalse(field(incognito = false) and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0)
    }

    /** One interceptor step: records the request it was handed on. */
    private class Step(
        override val request: ImageRequest,
    ) : Interceptor.Chain {
        var sent: ImageRequest? = null
        override val size: Size = Size.ORIGINAL

        override fun withRequest(request: ImageRequest): Interceptor.Chain = Step(request).also { next = it }

        override fun withSize(size: Size): Interceptor.Chain = this

        var next: Step? = null

        override suspend fun proceed(): ImageResult {
            sent = request
            return ErrorResult(null as Image?, request, IllegalStateException("test"))
        }
    }

    private fun policy(incognito: Boolean): CachePolicy {
        val request = ImageRequest.Builder(RuntimeEnvironment.getApplication()).data("https://img/tt1.jpg").build()
        val chain = Step(request)
        runBlocking { PrivateDisk { incognito }.intercept(chain) }
        return checkNotNull((chain.next ?: chain).sent).diskCachePolicy
    }

    @Test
    fun `images skip the disk cache in Incognito only`() {
        // DISABLED, not READ_ONLY: a read writes the URL's hash into the cache's journal.
        assertEquals(CachePolicy.DISABLED, policy(incognito = true))
        assertEquals(CachePolicy.ENABLED, policy(incognito = false))
    }

    @Test
    fun `the app's image loader carries the Incognito interceptor`() {
        val loader = createImageLoader(RuntimeEnvironment.getApplication()) { true }
        assertTrue(loader.components.interceptors.any { it is PrivateDisk })
    }

    @Test
    fun `the trailer host plays nothing in Incognito, so no player is made`() {
        val state = TrailerState()
        var ended = false
        TrailerHost(RuntimeEnvironment.getApplication(), off = true).play("dQw4w9WgXcQ", state) { ended = true }
        assertFalse(state.loading, "a loading state would compose the WebView's surface")
        assertFalse(ended)
        val on = TrailerState()
        TrailerHost(RuntimeEnvironment.getApplication()).play("dQw4w9WgXcQ", on) {}
        assertTrue(on.loading)
    }
}
