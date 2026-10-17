package app.marquee.android

import android.content.pm.ApplicationInfo
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.graphics.toArgb
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens

/**
 * The one activity. It decides the UI tree once (TV or phone, `FormFactor`), applies the theme
 * (TV is dark; a phone follows the system), and hands over to `MarqueeRoot`. Debug builds can
 * launch with extras that pick the tree and theme or open a debug screen (`DebugEntry`).
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val debuggable = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        val form = FormFactor.detect(this, if (debuggable) intent.getStringExtra("form") else null)
        val themeExtra = if (debuggable) intent.getStringExtra("theme") else null
        val forcedDark =
            when {
                form == Form.Tv -> true
                themeExtra == "light" -> false
                themeExtra == "dark" -> true
                else -> null
            }

        // Transparent bars; the icon color follows the theme the screen is drawn in.
        val bars = Color.TRANSPARENT
        val style = SystemBarStyle.auto(bars, bars, detectDarkMode = { forcedDark ?: it.isNight() })
        enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
        // The first frame shows this color, so a light phone never flashes dark.
        val dark = forcedDark ?: resources.isNight()
        window.setBackgroundDrawable(ColorDrawable((if (dark) Tokens.dark else Tokens.light).bg.toArgb()))

        if (DebugEntry.show(this, form, dark)) return
        val session = (application as MarqueeApp).session
        setContent {
            MarqueeTheme(form, forcedDark ?: isSystemInDarkTheme()) { MarqueeRoot(session, form) }
        }
    }

    override fun onDestroy() {
        DebugEntry.destroy()
        super.onDestroy()
    }
}

private fun android.content.res.Resources.isNight() =
    configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
