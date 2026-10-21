package app.marquee.ui.phone

import android.content.ComponentName
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.lifecycle.Lifecycle
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.LOADING_TAG
import app.marquee.player.PlaybackController
import app.marquee.player.PlayerLifecycle
import app.marquee.player.StartService
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * A start goes on while the app is away (plans/player-polish.md 2c), through the activity's own lifecycle: stopped
 * while the core resolves, the playback view stays open, a foreground service keeps the app running and the stream
 * loads with no sound; back in front, Ready goes on and it plays.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w844dp-h390dp")
class BackgroundStartTest {
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
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val sent = CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    /** The core's playback view, answering its subscribe with [first]. */
    private fun view(first: String): PlaybackView {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            msg["op"]?.jsonPrimitive?.content?.let { sent += it }
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$first}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        return PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() }
    }

    /** Lets the core's thread and the main looper run until [check] holds. */
    private fun until(check: () -> Boolean) =
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            check()
        }

    @Test
    fun `stopped while the core resolves, the start goes on in the background, and plays once back`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PlayerLifecycle(controller)
                PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {})
            }
        }
        compose.runOnIdle { controller.attach(view(RESOLVING)) }
        until { controller.subtitlesPending }
        compose.onNodeWithTag(LOADING_TAG).assertExists()
        // Home: the activity pauses, then stops.
        compose.activityRule.scenario.moveToState(Lifecycle.State.CREATED)
        assertTrue(StartService.keeping, "a foreground service keeps the app running")
        val asked = assertNotNull(shadowOf(compose.activity.application).nextStartedService)
        assertEquals(StartService::class.java.name, asked.component?.className)
        // The core's answer comes while away: the stream loads, paused.
        client.onMessage("""{"sub":7,"updates":[$READY]}""")
        until { controller.stage == PlaybackController.Stage.Buffering }
        assertFalse("unsubscribe" in sent, "the playback view stays open")
        assertFalse(player.playWhenReady, "no sound in the background")
        compose.runOnIdle { player.open() }
        until { controller.stage == PlaybackController.Stage.Paused }
        assertFalse(player.playWhenReady)
        assertFalse(StartService.keeping, "let go at Ready")
        // Back: Ready goes on, it plays at its point.
        compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        until { player.playWhenReady }
        assertEquals(60_000L, player.currentPosition)
    }

    @Test
    fun `back while still resolving, the service goes and the start goes on in front`() {
        compose.setContent { MarqueeTheme(Form.Phone) { PlayerLifecycle(controller) } }
        compose.runOnIdle { controller.attach(view(RESOLVING)) }
        until { controller.subtitlesPending }
        compose.activityRule.scenario.moveToState(Lifecycle.State.CREATED)
        assertTrue(StartService.keeping)
        compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        assertFalse(StartService.keeping, "the activity in front keeps the app running")
        assertFalse("unsubscribe" in sent)
    }

    @Test
    fun `the service is declared as media playback, with the permissions Android 14 and later ask for`() {
        val app = compose.activity.application
        val info = app.packageManager.getServiceInfo(ComponentName(app, StartService::class.java), 0)
        assertEquals(ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK, info.foregroundServiceType)
        assertFalse(info.exported)
        val asks =
            app.packageManager
                .getPackageInfo(
                    app.packageName,
                    PackageManager.GET_PERMISSIONS,
                ).requestedPermissions
                .orEmpty()
        for (p in listOf("FOREGROUND_SERVICE", "FOREGROUND_SERVICE_MEDIA_PLAYBACK", "WAKE_LOCK")) {
            assertTrue("android.permission.$p" in asks, "asks for $p: ${asks.toList()}")
        }
    }
}

private const val RESOLVING =
    """{"phase":"resolving","key":"k1","attempt":0,"manifest":false,"subtitlesPending":true,"trakt":"off","subtitles":[]}"""

private const val READY =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[]}"""
