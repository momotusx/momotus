package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp

/**
 * A profile's avatar (plans/m11-profiles.md, P1 apps D): a glyph or the name's [letter], white on
 * one of the avatar color tokens, in a circle (design/DECISION.md, Profiles). [glyph] and [color] are ids
 * already parsed (an unknown glyph is "letter", an unknown color null, drawn on `raised`). It is decoration:
 * the name beside it is what a screen reader reads.
 */
@Composable
fun AvatarArt(
    glyph: String,
    color: String?,
    letter: String,
    size: Dp,
    modifier: Modifier = Modifier,
    /** White with a dark glyph: a focused profile on TV. */
    lit: Boolean = false,
) {
    val colors = Marquee.colors
    val ink = if (lit) colors.onPrimary else Color.White
    Box(
        modifier
            .size(size)
            .clip(CircleShape)
            // Incognito's mark: the white hat-glasses on sea (plans/batch-10-incognito.md 5.2).
            .background(
                when {
                    lit -> colors.primary
                    glyph == INCOGNITO_GLYPH -> colors.tileSea
                    else -> avatarColor(color) ?: colors.raised
                },
            ).clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        val icon = avatarGlyph(glyph)
        if (icon == null) {
            BasicText(
                letter,
                style =
                    TextStyle(
                        color = ink,
                        fontSize = with(LocalDensity.current) { (size * 0.42f).toSp() },
                        fontFamily = MarqueeFonts.display,
                        fontWeight = FontWeight.ExtraBold,
                    ),
            )
        } else {
            MqIcon(icon, tint = ink, size = size * if (glyph == INCOGNITO_GLYPH) 0.6f else 0.46f)
        }
    }
}

/** The fill of an avatar color id, or null for an id this app does not know. */
@Composable
fun avatarColor(id: String?): Color? {
    val c = Marquee.colors
    return when (id) {
        "ember" -> c.avatarEmber
        "amber" -> c.avatarAmber
        "moss" -> c.avatarMoss
        "teal" -> c.avatarTeal
        "sky" -> c.avatarSky
        "plum" -> c.avatarPlum
        "rose" -> c.avatarRose
        "graphite" -> c.avatarGraphite
        else -> null
    }
}

/** Incognito's avatar id, as `ProfileCopy.INCOGNITO` stores it. */
const val INCOGNITO_GLYPH = "incognito"

private fun avatarGlyph(id: String): ImageVector? =
    when (id) {
        INCOGNITO_GLYPH -> Icons.HatGlasses
        "popcorn" -> Icons.Popcorn
        "clapperboard" -> Icons.Clapperboard
        "tv" -> Icons.Tv
        "rocket" -> Icons.Rocket
        "ghost" -> Icons.Ghost
        "cat" -> Icons.Cat
        "dog" -> Icons.Dog
        "leaf" -> Icons.Leaf
        "moon" -> Icons.Moon
        "flame" -> Icons.Flame
        "headphones" -> Icons.Headphones
        "gamepad" -> Icons.Gamepad2
        else -> null
    }
