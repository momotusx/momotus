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
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.core.graphics.drawable.toDrawable
import androidx.lifecycle.lifecycleScope
import app.marquee.core.ProfileCopy
import app.marquee.core.route.Route
import app.marquee.design.MarqueeTheme
import app.marquee.design.formPalette
import app.marquee.player.WalkLog
import app.marquee.player.WalkMute
import kotlinx.coroutines.launch

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
            app.marquee.design.HeroTrailers.walk = WalkMute.on
            app.marquee.design.HeroTrailers.walkSwitch = intent.getBooleanExtra("soundSwitch", false)
            WalkLog.read(this, intent)
        }
        DebugEntry.launched(this)
        if (DebugEntry.show(this, form)) return
        val session = (application as MarqueeApp).session
        // Incognito keeps its last frame out of Recents (plans/batch-10-incognito.md 5.5; Android 12 and older cannot).
        val gate = (application as MarqueeApp).profiles
        lifecycleScope.launch {
            snapshotFlow { ProfileCopy.inIncognito(gate.snapshot) }.collect { on ->
                recentsFrame(Build.VERSION.SDK_INT, on, ::recents)
            }
        }
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
        // A debug walk only: `--ei widthDp 360 --ef fontScale 1.3` lays the app out as a phone that wide at that font
        // size, so a walk checks a narrow screen and large text without touching the device's display or font settings.
        val widthDp = intent.getIntExtra("widthDp", 0).takeIf { debuggable }
        val fontScale = intent.getFloatExtra("fontScale", 0f).takeIf { debuggable }
        setContent {
            val asWalked = walkDensity(LocalDensity.current, resources.displayMetrics.widthPixels, widthDp, fontScale)
            CompositionLocalProvider(LocalDensity provides asWalked) {
                MarqueeTheme(form) { MarqueeRoot(session, form, debugStack) }
            }
        }
    }

    // One activity (singleTask): a link opened while the app runs comes here.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
            WalkMute.read(this, intent)
            app.marquee.design.HeroTrailers.walk = WalkMute.on
            app.marquee.design.HeroTrailers.walkSwitch = intent.getBooleanExtra("soundSwitch", false)
            WalkLog.read(this, intent)
        }
        addonLink(intent)?.let { (application as MarqueeApp).addonLink = it }
        accountIntent(intent)
    }

    /**
     * The account links (plans/accounts.md 4.2, 4.4): a Custom Tab's `momotus://auth` return goes to the sign-in
     * that waits (nothing happens when none does); a pairing code by link (the `/pair` App Link, or `momotus://pair`
     * from the link host's page) opens the approve sheet, and the Sign in another device shortcut the scanner.
     */
    private fun accountIntent(intent: Intent?) {
        val app = application as MarqueeApp
        when (val link = accountLink(intent)) {
            is AccountLink.Return -> {
                app.accountPlatform.customTabReturn(link.uri)
            }

            is AccountLink.Pair -> {
                // The newest code wins (plans/tv-qr-login.md 5.6); it lives in memory only.
                app.pendingCode = link.code
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

    private fun recents(enabled: Boolean) {
        if (Build.VERSION.SDK_INT >= 33) setRecentsScreenshotEnabled(enabled)
    }

    override fun onDestroy() {
        // Closing the app ends Incognito, also when a download keeps the process (plans/batch-10-incognito.md 5.3).
        if (isFinishing && !isChangingConfigurations) (application as MarqueeApp).profiles.leaveOnClose()
        DebugEntry.destroy()
        super.onDestroy()
    }
}

/** Recents shows the app's last frame only outside Incognito, from Android 13 (API 33), where [set] exists. */
internal fun recentsFrame(
    sdk: Int,
    incognito: Boolean,
    set: (Boolean) -> Unit,
) {
    if (sdk >= 33) set(!incognito)
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

    /**
     * A pairing code by link: `https://<link host>/pair#...` or `momotus://pair#...` (the link host's Open in
     * Momotus), whole, for the core to read. A link is never a scan (plans/tv-qr-login.md Q2).
     */
    data class Pair(
        val code: String,
    ) : AccountLink

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
            AccountLink.Pair(data.toString())
        }

        data.scheme == AccountPlatform.SCHEME && data.host == "pair" && data.path.isNullOrEmpty() -> {
            AccountLink.Pair(data.toString())
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

/**
 * The density a walk asks for: [base] with the width spread over [widthDp] and the font scaled to [fontScale], each
 * only when asked (positive); [base] itself otherwise.
 */
internal fun walkDensity(
    base: Density,
    widthPx: Int,
    widthDp: Int?,
    fontScale: Float?,
): Density {
    val density = widthDp?.takeIf { it > 0 }?.let { widthPx / it.toFloat() } ?: base.density
    val font = fontScale?.takeIf { it > 0f } ?: base.fontScale
    return if (density == base.density && font == base.fontScale) base else Density(density, font)
}
