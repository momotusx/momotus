package app.marquee.player

import android.app.Notification
import android.content.Intent
import android.os.Looper
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowPowerManager
import java.time.Duration
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/** The service that keeps a start going in the background (plans/player-polish.md 2c). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class StartServiceTest {
    private val app = RuntimeEnvironment.getApplication()

    @After
    fun stop() = StartService.release()

    /** What [StartService.keep] asked Android to start, then started as Android would. */
    private fun started(): StartService {
        val intent: Intent = assertNotNull(shadowOf(app).nextStartedService, "a service was asked for")
        assertEquals(StartService::class.java.name, intent.component?.className)
        return Robolectric
            .buildService(StartService::class.java, intent)
            .create()
            .startCommand(0, 1)
            .get()
    }

    @Test
    fun `kept, it runs in the foreground with its notification and locks, and is let go at once`() {
        StartService.keep(app, "Sintel")
        assertTrue(StartService.keeping)
        val service = started()
        val shadow = shadowOf(service)
        val notification = shadow.lastForegroundNotification
        assertEquals("Preparing Sintel", notification.extras.getCharSequence(Notification.EXTRA_TITLE).toString())
        assertTrue(notification.flags and Notification.FLAG_ONGOING_EVENT != 0)
        assertTrue(ShadowPowerManager.getLatestWakeLock().isHeld, "the CPU stays up with the screen off")
        StartService.release()
        assertFalse(StartService.keeping)
        assertTrue(shadow.isForegroundStopped && shadow.notificationShouldRemoved, "the notification goes")
        assertTrue(shadow.isStoppedBySelf)
    }

    @Test
    fun `let go before Android started it, it still starts in the foreground, then stops`() {
        StartService.keep(app, "Sintel")
        StartService.release()
        val shadow = shadowOf(started())
        // Android ends an app that asked for a foreground service and did not get one: it is started, then stopped.
        assertTrue(shadow.lastForegroundNotificationId != 0, "it went to the foreground")
        assertTrue(shadow.isForegroundStopped && shadow.isStoppedBySelf)
    }

    @Test
    fun `it ends itself after the limit, whatever the start does`() {
        StartService.keep(app, "Sintel")
        val shadow = shadowOf(started())
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(StartService.LIMIT_MS - 1_000))
        assertFalse(shadow.isStoppedBySelf)
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(2_000))
        assertTrue(shadow.isStoppedBySelf)
        assertFalse(StartService.keeping)
    }

    @Test
    fun `a title the player does not know yet reads as a plain sentence`() {
        assertEquals("Preparing to play", preparingWords(""))
        assertEquals("Preparing Night of the Living Dead", preparingWords("Night of the Living Dead"))
    }
}
