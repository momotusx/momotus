package app.marquee.player

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect

/**
 * The player as the app goes away and comes back (plans/player-polish.md 2c). Leaving stops the sound. A start under
 * way goes on: [StartService] keeps the app running from the moment the activity pauses (still in the foreground, as
 * Android allows it) until the start is over ([PlaybackController.starting]), the viewer is back, or the player
 * leaves; back on the screen it plays at its point.
 */
@Composable
fun PlayerLifecycle(controller: PlaybackController) {
    val context = LocalContext.current.applicationContext
    LifecycleEventEffect(
        Lifecycle.Event.ON_PAUSE,
    ) { if (controller.starting) StartService.keep(context, controller.title) }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { controller.onStop() }
    LifecycleEventEffect(Lifecycle.Event.ON_START) { controller.onStart() }
    // The activity in front keeps the app running by itself.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { StartService.release() }
    DisposableEffect(controller) { onDispose { StartService.release() } }
}
