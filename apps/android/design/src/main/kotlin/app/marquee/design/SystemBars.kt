package app.marquee.design

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private tailrec fun Context.activity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.activity()
        else -> null
    }

/**
 * While this is in the composition the status bar icons are light: the screen's artwork runs under the
 * bar, and the theme's dark icons (in light mode) would vanish on a dark picture. The theme's own
 * style comes back when the screen leaves.
 */
@Composable
fun LightStatusBarIcons() {
    val view = LocalView.current
    DisposableEffect(view) {
        val window = view.context.activity()?.window
        val controller = window?.let { WindowCompat.getInsetsController(it, view) }
        val previous = controller?.isAppearanceLightStatusBars
        controller?.isAppearanceLightStatusBars = false
        onDispose { if (previous != null) controller?.isAppearanceLightStatusBars = previous }
    }
}
