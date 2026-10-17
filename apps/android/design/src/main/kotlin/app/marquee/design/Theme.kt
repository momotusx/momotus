package app.marquee.design

import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
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

    /** Width of the focus halo: soft on dark, a crisp outer line on light (design/DECISION.md). */
    val focusHalo: Dp
        @Composable @ReadOnlyComposable
        get() = if (LocalPalette.current === Tokens.light) Tokens.focusHaloLight else Tokens.focusHaloDark

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
 * Over a picture (a hero, the player) the colors are the dark ones in both themes: light on dark, never the
 * other way. A secondary button in light mode would otherwise print dark text on a dark scrim.
 */
@Composable
fun OverPicture(content: @Composable () -> Unit) =
    CompositionLocalProvider(LocalPalette provides Tokens.dark, content = content)

/** Animations are off when the system animator scale is zero (Developer options, accessibility). */
@Composable
fun rememberReducedMotion(): Boolean {
    val resolver = LocalContext.current.contentResolver
    return remember(resolver) {
        runCatching { Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }
            .getOrDefault(false)
    }
}

/**
 * The theme for one UI tree. TV is dark; the phone tree follows [dark]. The TV tree is drawn on
 * the point canvas ([TvCanvas], applied here so no screen forgets it) and token sizes apply
 * unchanged.
 */
@Composable
fun MarqueeTheme(
    form: Form,
    dark: Boolean = true,
    content: @Composable () -> Unit,
) {
    val palette = if (dark) Tokens.dark else Tokens.light
    val themed: @Composable () -> Unit = {
        CompositionLocalProvider(
            LocalForm provides form,
            LocalPalette provides palette,
            LocalReducedMotion provides rememberReducedMotion(),
        ) {
            Box(Modifier.fillMaxSize().background(palette.bg).clipToBounds()) { content() }
        }
    }
    if (form == Form.Tv) TvCanvas(themed) else themed()
}
