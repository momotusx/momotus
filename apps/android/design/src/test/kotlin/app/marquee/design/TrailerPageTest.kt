package app.marquee.design

import android.app.ActivityManager
import android.net.ConnectivityManager
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createComposeRule
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertNotNull
import kotlin.test.assertNotSame
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** The app's one trailer player page, handed from screen to screen (plans/batch-13-perf.md, android-perf). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TrailerPageTest {
    private val context get() = RuntimeEnvironment.getApplication()

    @get:Rule
    val compose = createComposeRule()

    @After
    fun drop() = TrailerHost(context).dropPage()

    private fun device(totalMem: Long) {
        shadowOf(context.getSystemService(ActivityManager::class.java)).setMemoryInfo(
            ActivityManager.MemoryInfo().apply {
                this.totalMem = totalMem
                availMem = totalMem / 2
            },
        )
        // Robolectric's default network is mobile, which reads as metered.
        shadowOf(context.getSystemService(ConnectivityManager::class.java)).setDefaultNetworkActive(false)
    }

    /** Home's first frame with [host], then the main thread's first idle moment. */
    private fun warm(
        host: TrailerHost,
        wanted: Boolean,
    ) {
        screen = host to wanted
        compose.waitForIdle()
        shadowOf(Looper.getMainLooper()).idle()
    }

    private var screen by mutableStateOf<Pair<TrailerHost, Boolean>?>(null)

    private fun home() =
        compose.setContent {
            val (host, wanted) = screen ?: return@setContent
            key(host) { WarmTrailerPage(host, wanted) }
        }

    @Test
    fun `Home builds the player page when idle, before its first play`() {
        home()
        device(16_000_000_000L)
        warm(TrailerHost(context), wanted = true)
        assertNotNull(TrailerHost.Page.web, "the first play finds the page made")
    }

    @Test
    fun `no page is built where no preview may play`() {
        home()
        // A Fire TV stick under the phone floor, a Home with no trailer, Incognito.
        device(1_316_676L * 1024)
        warm(TrailerHost(context), wanted = true)
        assertNull(TrailerHost.Page.web)
        device(16_000_000_000L)
        warm(TrailerHost(context), wanted = false)
        assertNull(TrailerHost.Page.web)
        warm(TrailerHost(context, off = true), wanted = true)
        assertNull(TrailerHost.Page.web)
    }

    @Test
    fun `a second screen plays on the first screen's player, not a new one`() {
        val home = TrailerHost(context)
        home.play("dQw4w9WgXcQ", TrailerState()) {}
        val made = home.view()
        home.stop()
        val title = TrailerHost(context)
        title.play("dQw4w9WgXcQ", TrailerState()) {}
        assertSame(made, title.view(), "a WebView is built on the main thread: one per app, not one per visit")
    }

    @Test
    fun `a screen that goes ends only its own trailer`() {
        val home = TrailerHost(context)
        val homeState = TrailerState()
        home.play("dQw4w9WgXcQ", homeState) {}
        val title = TrailerHost(context)
        val titleState = TrailerState()
        title.play("dQw4w9WgXcQ", titleState) {}
        // Home leaves after the title page started its trailer on the same player.
        home.stop()
        assertTrue(titleState.loading, "the title's trailer plays on")
    }

    @Test
    fun `the player page goes when let go, and the next play makes a fresh one`() {
        val home = TrailerHost(context)
        var ended: String? = null
        home.play("dQw4w9WgXcQ", TrailerState()) { ended = it }
        val made = home.view()
        home.dropPage()
        assertTrue(ended == "stopped", "whoever plays hears it")
        home.play("dQw4w9WgXcQ", TrailerState()) {}
        assertNotSame(made, home.view())
    }
}
