package app.marquee.design

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Density

/** The TV canvas: tokens are points on 1920x1080. */
const val TV_CANVAS_WIDTH = 1920f
const val TV_CANVAS_HEIGHT = 1080f

/** Pixels per design point for a window of this size: the smaller axis ratio, so nothing overflows. */
fun tvScale(
    widthPx: Int,
    heightPx: Int,
): Float = minOf(widthPx / TV_CANVAS_WIDTH, heightPx / TV_CANVAS_HEIGHT).coerceAtLeast(0.1f)

/**
 * Makes 1 dp equal 1 design point on any TV UI buffer (720p, 1080p, or the 1080p buffer a 4K panel
 * scales up), and ignores the system font scale there. A wider or narrower window than 16:9 gets
 * more canvas on the long side instead of bars. Every overlay is composed inside this tree
 * rather than as a Dialog or Popup window, so it shares the density and the focus handling.
 */
@Composable
fun TvCanvas(content: @Composable () -> Unit) {
    val size = LocalWindowInfo.current.containerSize
    val scale = tvScale(size.width, size.height)
    val density = LocalDensity.current
    val canvas = if (size.width > 0 && size.height > 0) Density(scale, fontScale = 1f) else density
    CompositionLocalProvider(LocalDensity provides canvas) { content() }
}
