package app.marquee.ui.tv

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.unit.dp
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.LoadingLayer
import app.marquee.player.PlaybackController
import app.marquee.player.Scrim
import app.marquee.player.TrackChoice
import app.marquee.player.TvSubtitlePicker
import app.marquee.player.drawnBox
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import kotlin.coroutines.CoroutineContext

/**
 * Shots of the loading screen and the subtitle picker on a TV, for review: only with MARQUEE_SHOTS set to a folder
 * holding `art/logo.png` and `art/backdrop.jpg` (a title's logo and backdrop), into which they are written.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w960dp-h540dp-xhdpi")
class PlayerShotsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val out = System.getenv("MARQUEE_SHOTS")?.let(::File)

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

    @After
    fun stop() = scope.cancel()

    private fun art(name: String) = File(out, "art/$name")

    private fun logo() =
        BitmapFactory.decodeFile(art("logo.png").path).let { bitmap ->
            val pixels = IntArray(bitmap.width * bitmap.height)
            bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
            val box = drawnBox(pixels, bitmap.width, bitmap.height)!!
            Bitmap.createBitmap(bitmap, box.left, box.top, box.width, box.height).asImageBitmap()
        }

    private var current by mutableStateOf<(@Composable () -> Unit)?>(null)

    private fun show(content: @Composable () -> Unit) {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        if (current == null) {
            // Native graphics start the window in touch mode, where nothing takes focus; a TV has no touch mode.
            compose.runOnUiThread {
                compose.activity.window.decorView
                    .requestFocusFromTouch()
            }
            compose.setContent { MarqueeTheme(Form.Tv) { current?.invoke() } }
        }
        current = content
    }

    private fun save(name: String) {
        Thread.sleep(800)
        compose.waitForIdle()
        compose.mainClock.advanceTimeBy(1_000)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(out, "$name.jpg").outputStream().use { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }

    @Test
    fun loading() {
        listOf(0f, 0.4f, 0.8f).forEach { p ->
            show {
                LoadingLayer(
                    "Night of the Living Dead",
                    logo(),
                    art("backdrop.jpg").path,
                    p,
                    listOf("Starting 1080p, 1.5 GB from Torrentio"),
                )
            }
            save("tv-loading-${(p * 100).toInt()}")
        }
        show {
            LoadingLayer(
                "Night of the Living Dead",
                logo(),
                art("backdrop.jpg").path,
                0.1f,
                listOf("Starting 4K, 15.8 GB from Torrentio", "Finding peers, 3 so far"),
            )
        }
        save("tv-loading-wait")
        show {
            LoadingLayer(
                "Severance Mountain",
                null,
                art("backdrop.jpg").path,
                0.4f,
                listOf("Choosing the best version"),
            )
        }
        save("tv-loading-name")
    }

    @Test
    fun loadingInThePlayer() {
        val controller =
            PlaybackController(
                PlayingPlayer(),
                scope,
                "Night of the Living Dead",
                null,
                null,
                SystemClock.elapsedRealtime(),
            )
        controller.backdrop = art("backdrop.jpg").path
        controller.logo = art("logo.png").path
        controller.showStatus("Starting 1080p, 1.5 GB from Torrentio")
        show { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) }
        save("tv-loading-player")
        controller.release()
    }

    @Test
    fun subtitles() {
        show {
            Box(Modifier.fillMaxSize()) {
                Image(
                    BitmapFactory.decodeFile(art("backdrop.jpg").path).asImageBitmap(),
                    null,
                    Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
                Scrim(top = false, height = 440.dp, peak = 0.78f, modifier = Modifier.align(Alignment.BottomCenter))
                TvSubtitlePicker(
                    rows,
                    {},
                    Modifier.align(Alignment.TopEnd).padding(end = 80.dp, top = 96.dp),
                    timing = 0L,
                )
            }
        }
        save("tv-subtitles")
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
        save("tv-subtitles-arabic")
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionUp) }
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        save("tv-subtitle-options")
    }

    private fun lang(
        code: String,
        name: String,
        own: String?,
        count: Int,
    ) = (1..count).map { i ->
        TrackChoice(
            "late:$code$i",
            if (i ==
                1
            ) {
                "PSA, WEBRip 1080p"
            } else {
                "CAKES, WEB 1080p"
            },
            "OpenSubtitles v3",
            section = name,
            language = code,
            nativeName = own,
        )
    }

    /** Slow Horses S6:E2 from OpenSubtitles and the file's English, as the controller lists them. */
    private val rows =
        listOf(TrackChoice("off", "Off")) +
            listOf(
                TrackChoice(
                    "late:en1",
                    "CAKES, WEB 1080p",
                    "OpenSubtitles v3",
                    section = "English",
                    language = "en",
                    release = "slow.horses.s06e02.1080p.web.h264-cakes",
                ),
                TrackChoice(
                    "late:en2",
                    "PSA, WEBRip 1080p 10 bit",
                    "OpenSubtitles v3",
                    section = "English",
                    language = "en",
                    release = "Slow.Horses.S06E02.1080p.WEBRip.6CH.x265.HEVC-PSA",
                ),
                TrackChoice(
                    "late:en3",
                    "CAKES, WEB 1080p",
                    "OpenSubtitles v3 · For the hard of hearing",
                    section = "English",
                    language = "en",
                    release = "slow.horses.s06e02.1080p.web.h264-cakes-hi",
                ),
                TrackChoice(
                    "late:en4",
                    "FLUX, WEB-DL 1080p",
                    "OpenSubtitles v3",
                    section = "English",
                    language = "en",
                    fits = true,
                    release = "Slow.Horses.S06E02.Daddy.Issues.1080p.ATVP.WEB-DL.DDP5.1.Atmos.H.264-FLUX",
                ),
            ) +
            lang("ar", "Arabic", "العربية", 3) +
            lang("pt-br", "Portuguese (Brazil)", "Português (Brasil)", 1) +
            lang("bg", "Bulgarian", "Български", 2) +
            lang("zh", "Chinese", "中文", 1) +
            lang("hr", "Croatian", "Hrvatski", 2) +
            lang("cs", "Czech", "Čeština", 1) +
            lang("da", "Danish", "Dansk", 1) +
            lang("nl", "Dutch", "Nederlands", 2) +
            lang("et", "Estonian", "Eesti", 1) +
            listOf(
                TrackChoice(
                    "0:0",
                    "English",
                    "In the file",
                    selected = true,
                    section = "English",
                    language = "en",
                    inFile = true,
                ),
            )
}
