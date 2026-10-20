package app.marquee.design

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp

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
 * scales up). The system text size holds up to [TV_MAX_FONT_SCALE] (lane heights follow the type
 * roles, so larger words get taller lanes). A wider or narrower window than 16:9 gets more canvas on
 * the long side instead of bars. Every overlay is composed inside this tree rather than as a Dialog
 * or Popup window, so it shares the density and the focus handling.
 */
@Composable
fun TvCanvas(content: @Composable () -> Unit) {
    val window = windowPx()
    val text = tvFontScale(LocalDensity.current.fontScale)
    CompositionLocalProvider(LocalDensity provides Density(tvScale(window.width, window.height), fontScale = text)) {
        content()
    }
}

/** The largest system text size a TV honors: a 10-foot layout keeps its rows on screen. */
const val TV_MAX_FONT_SCALE = 1.3f

/** The system text size on a TV, up to [TV_MAX_FONT_SCALE]. */
fun tvFontScale(system: Float): Float = system.coerceAtMost(TV_MAX_FONT_SCALE)

/** The window in pixels; the display's size on the first frame, when the window can still say 0x0. */
@Composable
fun windowPx(): IntSize {
    val window = LocalWindowInfo.current.containerSize
    val screen = LocalResources.current.displayMetrics
    return IntSize(
        if (window.width > 0) window.width else screen.widthPixels,
        if (window.height > 0) window.height else screen.heightPixels,
    )
}

/** The window in dp at the display's own density (what Configuration's screen size said), also inside [TvCanvas]. */
@Composable
fun windowDp(): DpSize {
    val px = windowPx()
    val density = LocalResources.current.displayMetrics.density
    return DpSize((px.width / density).dp, (px.height / density).dp)
}
