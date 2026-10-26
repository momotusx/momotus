package app.marquee.android

import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.ApplicationInfo
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.ui.graphics.toArgb
import androidx.core.graphics.drawable.toDrawable
import app.marquee.core.route.Route
import app.marquee.design.MarqueeTheme
import app.marquee.design.formPalette
import app.marquee.player.WalkLog
import app.marquee.player.WalkMute

/**
 * The one activity. It decides the UI tree once (TV or phone, `FormFactor`), applies the theme
 * (dark on every form), and hands over to `MarqueeRoot`. Debug builds can launch with extras that
 * pick the tree or open a debug screen (`DebugEntry`).
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The Auth Tab's launcher is registered before the activity starts, as the result API needs.
        (application as MarqueeApp).accountPlatform.attach(this)
        val debuggable = applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        // Debug and benchmark builds take the tree from `form=tv` (the TV benchmarks run on a phone).
        val asked = intent.getStringExtra("form")?.takeIf { debuggable || BuildConfig.BENCHMARK }
        val form = FormFactor.detect(this, asked)
        // A walk's landscape window, or a TV benchmark on a phone: the TV canvas in a landscape window, as on a TV,
        // not shrunk into portrait where every lane fits and nothing scrolls. The app asks; the device's own
        // rotation setting is untouched.
        if (intent.walkLandscape(debuggable) || (BuildConfig.BENCHMARK && asked == "tv")) {
            requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        }
        // A debug walk build (its own package: never the owner's app, never a release) holds the screen on while
        // it is in front, so the phone does not lock between a walk's steps; the device's settings are untouched.
        if (BuildConfig.WALK) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        applyBars()
        // The first frame shows the form's page color.
        window.setBackgroundDrawable(
            formPalette(form)
                .bg
                .toArgb()
                .toDrawable(),
        )

        if (debuggable) {
            WalkMute.read(this, intent)
            WalkLog.read(this, intent)
        }
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
        if (savedInstanceState == null) {
            addonLink(intent)?.let { (application as MarqueeApp).addonLink = it }
            accountIntent(intent)
        }
        setContent {
            MarqueeTheme(form) { MarqueeRoot(session, form, debugStack) }
        }
    }

    // One activity (singleTask): a link opened while the app runs comes here.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
            WalkMute.read(this, intent)
            WalkLog.read(this, intent)
        }
        addonLink(intent)?.let { (application as MarqueeApp).addonLink = it }
        accountIntent(intent)
    }

    /**
     * The account links (plans/accounts.md 4.2, 4.4): a Custom Tab's `momotus://auth` return goes to the sign-in
     * that waits (nothing happens when none does); the `/pair` App Link and the Sign in another device shortcut
     * open the scanner.
     */
    private fun accountIntent(intent: Intent?) {
        val app = application as MarqueeApp
        when (val link = accountLink(intent)) {
            is AccountLink.Return -> {
                app.accountPlatform.customTabReturn(link.uri)
            }

            AccountLink.Pair -> {
                app.scanAgain = true
                app.pendingScan = true
            }

            AccountLink.Shortcut -> {
                app.pendingScan = true
            }

            null -> {
                Unit
            }
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

/** An account link the activity was opened with. */
internal sealed interface AccountLink {
    data class Return(
        val uri: Uri,
    ) : AccountLink

    /** `https://<link host>/pair#...`: the fragment is never read (D6). */
    data object Pair : AccountLink

    data object Shortcut : AccountLink
}

/** The app shortcut's action (`res/xml/shortcuts.xml`). */
internal const val SIGN_IN_ANOTHER = "app.momotus.android.SIGN_IN_ANOTHER"

internal fun accountLink(
    intent: Intent?,
    linkHost: String = BuildConfig.LINK_HOST,
): AccountLink? {
    if (intent == null || (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0) return null
    if (intent.action == SIGN_IN_ANOTHER) return AccountLink.Shortcut
    if (intent.action != Intent.ACTION_VIEW) return null
    val data = intent.data ?: return null
    return when {
        data.scheme == AccountPlatform.SCHEME && data.host == "auth" -> {
            AccountLink.Return(data)
        }

        linkHost.isNotEmpty() && data.scheme == "https" && data.host == linkHost && data.path == "/pair" -> {
            AccountLink.Pair
        }

        else -> {
            null
        }
    }
}

/**
 * A walk of the TV form on a phone (`--es form tv --ez landscape true`, debug builds only): the app asks for a
 * landscape window itself, so the device's own rotation setting is never touched.
 */
internal fun Intent.walkLandscape(debuggable: Boolean): Boolean = debuggable && getBooleanExtra("landscape", false)
