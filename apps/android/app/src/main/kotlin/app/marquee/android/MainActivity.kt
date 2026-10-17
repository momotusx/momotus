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
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens

/**
 * The one activity. It decides the UI tree once (TV or phone, `FormFactor`), applies the theme
 * (TV is dark; a phone follows the system), and hands over to `MarqueeRoot`. Debug builds can
 * launch with extras that pick the tree and theme or open a debug screen (`DebugEntry`).
 */
class MainActivity : ComponentActivity() {
    /** Set for a TV and for a debug launch that names a theme; null means the phone follows the system. */
    private var forcedDark: Boolean? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val debuggable = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        val form = FormFactor.detect(this, if (debuggable) intent.getStringExtra("form") else null)
        val themeExtra = if (debuggable) intent.getStringExtra("theme") else null
        forcedDark =
            when {
                form == Form.Tv -> true
                themeExtra == "light" -> false
                themeExtra == "dark" -> true
                else -> null
            }
        val forcedDark = forcedDark

        applyBars()
        // The first frame shows this color, so a light phone never flashes dark.
        val dark = forcedDark ?: resources.isNight()
        window.setBackgroundDrawable(ColorDrawable((if (dark) Tokens.dark else Tokens.light).bg.toArgb()))

        DebugEntry.launched(this)
        if (DebugEntry.show(this, form, dark)) return
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
            MarqueeTheme(form, forcedDark ?: isSystemInDarkTheme()) { MarqueeRoot(session, form, debugStack) }
        }
    }

    // The manifest handles uiMode changes without recreating the activity, so a phone that
    // switches between light and dark while the app is open needs its bar icons set again.
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        applyBars()
    }

    /** Transparent bars; the icon color follows the theme the screen is drawn in. */
    private fun applyBars() {
        val bars = Color.TRANSPARENT
        val forced = forcedDark
        val style = SystemBarStyle.auto(bars, bars, detectDarkMode = { forced ?: it.isNight() })
        enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
    }

    override fun onDestroy() {
        DebugEntry.destroy()
        super.onDestroy()
    }
}

private fun android.content.res.Resources.isNight() =
    configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
