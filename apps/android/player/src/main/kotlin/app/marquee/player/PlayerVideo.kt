package app.marquee.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.Player
import androidx.media3.common.text.CueGroup
import androidx.media3.ui.SubtitleView
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_SURFACE_VIEW

/**
 * The picture and its subtitles. The picture goes to a SurfaceView, which is what lets the
 * platform switch a TV to an HDR mode; a TextureView would tone-map everything to SDR. The frame
 * fits the picture to the screen at its own shape (black bars where the shapes differ). The
 * subtitles follow the system caption settings (size, color, edge) until the app has its own.
 *
 * On the emulator's phone screen a 1080p picture shows at its native 1920 px in the middle of a
 * 2400 px screen, with or without this frame doing the sizing (checked with a hand-sized
 * PlayerSurface too): the layout is full width, so that is the emulator's video layer, which
 * does not scale up here. A real display is the thing to check it on.
 */
@Composable
fun PlayerVideo(
    player: Player,
    modifier: Modifier = Modifier,
) {
    Box(modifier.background(Color.Black)) {
        ContentFrame(
            player = player,
            modifier = Modifier.fillMaxSize(),
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
        )
        Subtitles(player, Modifier.fillMaxSize())
    }
}

@Composable
private fun Subtitles(
    player: Player,
    modifier: Modifier,
) {
    var view by remember { mutableStateOf<SubtitleView?>(null) }
    AndroidView(
        factory = { context ->
            SubtitleView(context).also {
                it.setUserDefaultStyle()
                it.setUserDefaultTextSize()
                view = it
            }
        },
        modifier = modifier,
    )
    DisposableEffect(player, view) {
        val target = view
        val listener =
            object : Player.Listener {
                override fun onCues(cueGroup: CueGroup) {
                    target?.setCues(cueGroup.cues)
                }
            }
        target?.setCues(player.currentCues.cues)
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }
}
