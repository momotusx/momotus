package app.marquee.android

import android.content.Intent
import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.graphics.toArgb
import androidx.core.graphics.drawable.toDrawable
import app.marquee.core.route.Route
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens
import app.marquee.player.WalkMute

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
        window.setBackgroundDrawable(
            Tokens.dark.bg
                .toArgb()
                .toDrawable(),
        )

        if (debuggable) WalkMute.read(this, intent)
        DebugEntry.launched(this)
        if (DebugEntry.show(this, form)) return
        val session = (application as MarqueeApp).session
        // Debug and benchmark builds (never a store build): `title=movie:tt1727587` starts on that title page,
        // with Home under it.
        val debugStack =
            if (debuggable || BuildConfig.BENCHMARK) {
                intent.getStringExtra("title")?.split(":", limit = 2)?.takeIf { it.size == 2 }?.let {
                    listOf(Route.Home, Route.Title(it[0], it[1]))
                }
            } else {
                null
            }
        // Once: a recreated activity (after the process died, a font size or language change) gets the
        // same intent again, and the link was taken already.
        if (savedInstanceState == null) addonLink(intent)?.let { (application as MarqueeApp).addonLink = it }
        setContent {
            MarqueeTheme(form) { MarqueeRoot(session, form, debugStack) }
        }
    }

    // One activity (singleTask): a link opened while the app runs comes here.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) WalkMute.read(this, intent)
        addonLink(intent)?.let { (application as MarqueeApp).addonLink = it }
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

/**
 * The addon link of an Install button (`stremio://host/.../manifest.json`) the activity was opened with, else
 * null: another stremio:// link (a title page, `stremio:///detail/...`) is not an addon, and a launch from the
 * recent apps repeats an old intent.
 */
internal fun addonLink(intent: Intent?): String? =
    intent
        ?.takeIf { it.action == Intent.ACTION_VIEW && (it.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) == 0 }
        ?.data
        ?.takeIf { it.scheme.equals("stremio", ignoreCase = true) && it.path.orEmpty().endsWith("manifest.json") }
        ?.toString()
