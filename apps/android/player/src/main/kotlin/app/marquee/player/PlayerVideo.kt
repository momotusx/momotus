package app.marquee.player

import android.content.Context
import android.view.accessibility.CaptioningManager
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
import app.marquee.core.model.SubtitleSize

/**
 * The picture and its subtitles. The picture goes to a SurfaceView, which is what lets the
 * platform switch a TV to an HDR mode; a TextureView would tone-map everything to SDR. The frame
 * fits the picture to the screen at its own shape (black bars where the shapes differ). The
 * subtitles follow the system caption settings (color, edge, and size), their size scaled by the
 * profile's subtitle size ([subtitleScale]).
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
    subtitleSize: SubtitleSize = SubtitleSize.Medium,
) {
    Box(modifier.background(Color.Black)) {
        ContentFrame(
            player = player,
            modifier = Modifier.fillMaxSize(),
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
        )
        Subtitles(player, subtitleScale(subtitleSize), Modifier.fillMaxSize())
    }
}

/** The profile's subtitle size as a factor on the system caption size (Medium leaves it as it is). */
fun subtitleScale(size: SubtitleSize): Float =
    when (size) {
        SubtitleSize.Small -> 0.75f
        SubtitleSize.Large -> 1.3f
        SubtitleSize.ExtraLarge -> 1.6f
        SubtitleSize.Medium, SubtitleSize.Unknown -> 1f
    }

@Composable
private fun Subtitles(
    player: Player,
    scale: Float,
    modifier: Modifier,
) {
    var view by remember { mutableStateOf<SubtitleView?>(null) }
    AndroidView(
        factory = { context ->
            SubtitleView(context).also {
                it.setUserDefaultStyle()
                view = it
            }
        },
        // The system caption size (as setUserDefaultTextSize reads it) times the profile's size.
        update = {
            it.setFractionalTextSize(
                SubtitleView.DEFAULT_TEXT_SIZE_FRACTION * captionScale(it.context) * scale,
            )
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

/** The system caption text size, 1 when captions are off in the system settings (as SubtitleView reads it). */
private fun captionScale(context: Context): Float {
    val captions = context.getSystemService(CaptioningManager::class.java) ?: return 1f
    return if (captions.isEnabled) captions.fontScale else 1f
}
