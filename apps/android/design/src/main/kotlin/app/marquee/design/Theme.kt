package app.marquee.design

import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.Dp

/** Which UI tree is showing: the TV tree reads tokens as points on a 1920x1080 canvas. */
enum class Form { Tv, Phone }

val LocalForm = staticCompositionLocalOf { Form.Phone }
val LocalPalette = staticCompositionLocalOf { Tokens.dark }
val LocalReducedMotion = staticCompositionLocalOf { false }

/** The session is in Incognito (plans/batch-10-incognito.md 5.5): no keyboard learning, no trailer, no outside link. */
val LocalIncognito = staticCompositionLocalOf { false }

/** Reads of the current theme, e.g. `Marquee.colors.accent`. */
object Marquee {
    val form: Form
        @Composable @ReadOnlyComposable
        get() = LocalForm.current

    val colors: Palette
        @Composable @ReadOnlyComposable
        get() = LocalPalette.current

    val type: TypeScale
        @Composable @ReadOnlyComposable
        get() = if (LocalForm.current == Form.Tv) Tokens.tvType else Tokens.phoneType

    val tiles: TileMetrics
        @Composable @ReadOnlyComposable
        get() = if (LocalForm.current == Form.Tv) Tokens.tvTiles else Tokens.phoneTiles

    val safe: Insets
        @Composable @ReadOnlyComposable
        get() = if (LocalForm.current == Form.Tv) Tokens.tvSafe else Tokens.phoneSafe

    /** Width of the focus halo around the ring (design/DECISION.md). */
    val focusHalo: Dp get() = Tokens.focusHaloDark

    val space: Spacing get() = Tokens.space
    val radius: Radii get() = Tokens.radius

    /** Duration for focus changes: zero when the user turned animations off. */
    val focusMs: Int
        @Composable @ReadOnlyComposable
        get() = if (LocalReducedMotion.current) 0 else Tokens.FOCUS_MS

    val panelMs: Int
        @Composable @ReadOnlyComposable
        get() = if (LocalReducedMotion.current) 0 else Tokens.PANEL_MS
}

/**
 * A debug walk's `reduceMotion` launch extra (plans/first-screens.md 10): reduced motion without changing the
 * device's settings. Set once by the activity of a debuggable build, before the first frame.
 */
var launchReducedMotion = false

/** Animations are off when the system animator scale is zero (Developer options), or by [launchReducedMotion]. */
@Composable
fun rememberReducedMotion(): Boolean {
    val resolver = LocalContext.current.contentResolver
    return remember(resolver) {
        launchReducedMotion ||
            runCatching { Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
                .getOrDefault(false)
    }
}

/**
 * A form's colors (design/DECISION.md, Page): the TV page is systemGray6; the phone is the iPhone's, a
 * black page with #1C1C1E groups, #2C2C2E inside them, iOS's grey for secondary text and the #F5F5F5 pill.
 */
fun formPalette(form: Form): Palette =
    when (form) {
        Form.Tv -> {
            Tokens.dark
        }

        Form.Phone -> {
            Tokens.dark.run {
                copy(bg = bgPhone, surface = surfacePhone, raised = raisedPhone, dim = dimPhone, primary = primaryPhone)
            }
        }
    }

/**
 * The theme for one UI tree, dark on every form, in its form's colors ([formPalette]). The TV tree is
 * drawn on the point canvas ([TvCanvas], applied here so no screen forgets it) and token sizes apply
 * unchanged.
 */
@Composable
fun MarqueeTheme(
    form: Form,
    content: @Composable () -> Unit,
) {
    val palette = formPalette(form)
    val themed: @Composable () -> Unit = {
        CompositionLocalProvider(
            LocalForm provides form,
            LocalPalette provides palette,
            LocalReducedMotion provides rememberReducedMotion(),
            LocalButtonFocus provides remember { mutableIntStateOf(0) },
        ) {
            Box(Modifier.fillMaxSize().background(palette.bg).clipToBounds()) { content() }
        }
    }
    if (form == Form.Tv) TvCanvas(themed) else themed()
}
