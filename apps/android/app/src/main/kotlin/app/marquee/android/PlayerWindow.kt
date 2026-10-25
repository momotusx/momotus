package app.marquee.android

import android.app.PictureInPictureParams
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.database.ContentObserver
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Rational
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.annotation.RequiresApi
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.util.Consumer
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage
import app.marquee.player.ScreenBrightness

/** What the activity needs from the player screen before Android 12, where it enters picture-in-picture itself. */
object PipHooks {
    @Volatile
    var enter: (() -> Unit)? = null
}

/** The window state of the player screen, read by the phone UI. */
@Stable
class PlayerWindow(
    private val activity: ComponentActivity?,
    private val controller: PlaybackController,
) {
    var inPip by mutableStateOf(activity?.isInPictureInPictureMode == true)
        internal set

    /** The window's brightness as the phone player's swipe sets it; one for every player. */
    val brightness: ScreenBrightness get() = screenBrightness

    fun enterPip() {
        val host = activity ?: return
        if (Build.VERSION.SDK_INT >= 26 && controller.stage != Stage.Failed) {
            runCatching { host.enterPictureInPictureMode(pipParams(controller, autoEnter = false)) }
        }
    }
}

/** The small window keeps the picture's shape, within the limits Android takes (0.418 to 2.39). */
@RequiresApi(Build.VERSION_CODES.O)
private fun pipParams(
    controller: PlaybackController,
    autoEnter: Boolean,
): PictureInPictureParams {
    val ratio = controller.videoRatio.takeIf { it > 0f } ?: (16f / 9f)
    val builder =
        PictureInPictureParams
            .Builder()
            .setAspectRatio(Rational((ratio.coerceIn(0.418f, 2.39f) * 1000).toInt(), 1000))
    if (Build.VERSION.SDK_INT >= 31) builder.setAutoEnterEnabled(autoEnter)
    return builder.build()
}

private tailrec fun Context.activity(): ComponentActivity? =
    when (this) {
        is ComponentActivity -> this
        is ContextWrapper -> baseContext.activity()
        else -> null
    }

/**
 * The player entry the window belongs to. A player that replaces a player (the next episode, an offered
 * version) comes in while the old one fades out: the old one's leaving, and what it still sets on its
 * way out, must not undo what the new one set.
 */
internal class WindowClaim {
    private var owner: Any? = null

    fun claim(entry: Any) {
        owner = entry
    }

    fun owns(entry: Any): Boolean = owner === entry

    fun release(entry: Any) {
        if (owner === entry) owner = null
    }
}

private val windowClaim = WindowClaim()

/** One for every player, so the next episode starts from the viewer's brightness (plans/player-gestures.md 3.5). */
private val screenBrightness = ScreenBrightness()

/**
 * What the window does while the player is up. On every form the screen stays awake while
 * something plays or loads and the system bars are hidden. On a phone the player also locks to
 * landscape and goes to a small picture-in-picture window when the viewer leaves while it plays (Android 12
 * and later do that by themselves once told; before it, the activity asks through [PipHooks]). A brightness
 * the viewer swiped holds in the window until the player leaves, the small window opens (it comes back after)
 * or the system's brightness changes, so its own slider is never dead in the player. All of it is put back when the player leaves, unless another player has taken the window ([WindowClaim]).
 */
@Composable
fun rememberPlayerWindow(
    controller: PlaybackController,
    phone: Boolean,
): PlayerWindow {
    val context = LocalContext.current
    val view = LocalView.current
    val activity = remember(context) { context.activity() }
    val window = remember(activity, controller) { PlayerWindow(activity, controller) }
    val entry = remember { Any() }
    val owns = { windowClaim.owns(entry) }
    // First, so it is disposed last: the effects below still own the window while they put it back.
    DisposableEffect(entry) {
        windowClaim.claim(entry)
        onDispose { windowClaim.release(entry) }
    }

    val awake = controller.stage != Stage.Paused && controller.stage != Stage.Ended && controller.stage != Stage.Failed
    DisposableEffect(awake) {
        if (owns()) view.keepScreenOn = awake
        onDispose { if (owns()) view.keepScreenOn = false }
    }

    if (activity != null) {
        // Immersive on every form: nothing but the picture and our own controls.
        DisposableEffect(activity) {
            val bars = WindowCompat.getInsetsController(activity.window, view)
            if (owns()) {
                bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                bars.hide(WindowInsetsCompat.Type.systemBars())
            }
            onDispose { if (owns()) bars.show(WindowInsetsCompat.Type.systemBars()) }
        }
    }
    if (phone && activity != null) {
        DisposableEffect(activity) {
            if (owns()) activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
            onDispose { if (owns()) activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED }
        }
        DisposableEffect(activity) {
            val system =
                object : ContentObserver(Handler(Looper.getMainLooper())) {
                    override fun onChange(selfChange: Boolean) {
                        // Only the viewer's own change counts: one while our override is on the window and the
                        // player is in front. With it dropped (picture in picture, the background) the panel follows
                        // the system, which then writes the setting itself under adaptive brightness (bench PGA9):
                        // the held level stays, to come back with the player.
                        val override = activity.window.attributes.screenBrightness
                        val front = activity.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
                        if (owns() && front && override != WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE) {
                            screenBrightness.release(activity.window, forget = true)
                        }
                    }
                }
            activity.contentResolver.registerContentObserver(
                Settings.System.getUriFor(Settings.System.SCREEN_BRIGHTNESS),
                false,
                system,
            )
            onDispose {
                activity.contentResolver.unregisterContentObserver(system)
                if (owns()) screenBrightness.release(activity.window, forget = true)
            }
        }
        if (Build.VERSION.SDK_INT >= 26) {
            val playing = controller.stage == Stage.Playing
            val ratio = controller.videoRatio
            DisposableEffect(activity, playing, ratio) {
                if (owns()) {
                    runCatching { activity.setPictureInPictureParams(pipParams(controller, autoEnter = playing)) }
                    PipHooks.enter = { if (playing) window.enterPip() }
                }
                onDispose { if (owns()) PipHooks.enter = null }
            }
            DisposableEffect(activity) {
                val listener =
                    Consumer<PictureInPictureModeChangedInfo> {
                        window.inPip = it.isInPictureInPictureMode
                        // The small window must not dim or light the screen under other apps.
                        if (owns()) {
                            if (it.isInPictureInPictureMode) {
                                screenBrightness.release(activity.window, forget = false)
                            } else {
                                screenBrightness.apply(activity.window)
                            }
                        }
                    }
                activity.addOnPictureInPictureModeChangedListener(listener)
                onDispose {
                    activity.removeOnPictureInPictureModeChangedListener(listener)
                    if (Build.VERSION.SDK_INT >= 31 && owns()) {
                        // Leaving the player must not send the next screen to the small window.
                        runCatching {
                            activity.setPictureInPictureParams(
                                PictureInPictureParams.Builder().setAutoEnterEnabled(false).build(),
                            )
                        }
                    }
                }
            }
        }
    }
    return window
}
