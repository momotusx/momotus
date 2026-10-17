package app.marquee.android

import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.graphics.toArgb
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens

/**
 * The one activity. It decides the UI tree once (TV or phone, `FormFactor`), applies the theme
 * (dark on every form), and hands over to `MarqueeRoot`. Debug builds can launch with extras that
 * pick the tree or open a debug screen (`DebugEntry`).
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val debuggable = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        val form = FormFactor.detect(this, if (debuggable) intent.getStringExtra("form") else null)
        applyBars()
        // The first frame shows the page color.
        window.setBackgroundDrawable(ColorDrawable(Tokens.dark.bg.toArgb()))

        DebugEntry.launched(this)
        if (DebugEntry.show(this, form)) return
        val session = (application as MarqueeApp).session
        // Debug: `title=movie:tt1727587` starts on that title page, with Home under it.
        val debugStack =
            if (debuggable) {
                intent.getStringExtra("title")?.split(":", limit = 2)?.takeIf { it.size == 2 }?.let {
                    listOf(Route.Home, Route.Title(it[0], it[1]))
                }
            } else {
                null
            }
        setContent {
            MarqueeTheme(form) { MarqueeRoot(session, form, debugStack) }
        }
    }

    /** Transparent bars with light icons: every screen is dark. */
    private fun applyBars() {
        val style = SystemBarStyle.dark(Color.TRANSPARENT)
        enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
    }

    // Before Android 12 the activity enters picture-in-picture itself when the viewer leaves while
    // a video plays; from Android 12 the player screen has told the system to do it.
    override fun onUserLeaveHint() {
        super.onUserLeaveHint()
        if (Build.VERSION.SDK_INT < 31) PipHooks.enter?.invoke()
    }

    override fun onDestroy() {
        DebugEntry.destroy()
        super.onDestroy()
    }
}
