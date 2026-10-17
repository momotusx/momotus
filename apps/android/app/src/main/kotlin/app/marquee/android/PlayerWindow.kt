package app.marquee.android

import android.app.PictureInPictureParams
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.os.Build
import android.util.Rational
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
import app.marquee.player.PlaybackController
import app.marquee.player.PlaybackController.Stage

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
 * What the window does while the player is up. On every form the screen stays awake while
 * something plays or loads and the system bars are hidden. On a phone the player also locks to
 * landscape and goes to a small picture-in-picture window when the viewer leaves while it plays (Android 12
 * and later do that by themselves once told; before it, the activity asks through [PipHooks]).
 * All of it is put back when the player leaves.
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

    val awake = controller.stage != Stage.Paused && controller.stage != Stage.Ended && controller.stage != Stage.Failed
    DisposableEffect(awake) {
        view.keepScreenOn = awake
        onDispose { view.keepScreenOn = false }
    }

    if (activity != null) {
        // Immersive on every form: nothing but the picture and our own controls.
        DisposableEffect(activity) {
            val bars = WindowCompat.getInsetsController(activity.window, view)
            bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            bars.hide(WindowInsetsCompat.Type.systemBars())
            onDispose { bars.show(WindowInsetsCompat.Type.systemBars()) }
        }
    }
    if (phone && activity != null) {
        DisposableEffect(activity) {
            activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
            onDispose { activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED }
        }
        if (Build.VERSION.SDK_INT >= 26) {
            val playing = controller.stage == Stage.Playing
            val ratio = controller.videoRatio
            DisposableEffect(activity, playing, ratio) {
                runCatching { activity.setPictureInPictureParams(pipParams(controller, autoEnter = playing)) }
                PipHooks.enter = { if (playing) window.enterPip() }
                onDispose { PipHooks.enter = null }
            }
            DisposableEffect(activity) {
                val listener = Consumer<PictureInPictureModeChangedInfo> { window.inPip = it.isInPictureInPictureMode }
                activity.addOnPictureInPictureModeChangedListener(listener)
                onDispose {
                    activity.removeOnPictureInPictureModeChangedListener(listener)
                    if (Build.VERSION.SDK_INT >= 31) {
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
