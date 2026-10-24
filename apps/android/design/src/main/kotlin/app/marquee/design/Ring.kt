package app.marquee.design

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The determinate twin of [Spinner] (plans/offline-downloads.md 3.1): a download's progress from the top, clockwise,
 * white on its 10 percent track, never a hue. 28 dp with a 2.5 dp stroke unless the caller sizes it.
 */
@Composable
fun ProgressRing(
    progress: Float,
    modifier: Modifier = Modifier,
    size: Dp = 28.dp,
    stroke: Dp = 2.5.dp,
    color: Color = Marquee.colors.text,
) {
    Canvas(modifier.size(size)) {
        val width = stroke.toPx()
        val inset = Offset(width / 2, width / 2)
        val box = Size(this.size.width - width, this.size.height - width)
        drawArc(color.copy(alpha = 0.1f), 0f, 360f, false, inset, box, style = Stroke(width))
        val sweep = 360f * progress.coerceIn(0f, 1f)
        if (sweep > 0f) drawArc(color, -90f, sweep, false, inset, box, style = Stroke(width, cap = StrokeCap.Round))
    }
}
