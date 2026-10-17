package app.marquee.design

import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit

/**
 * Fraunces (display titles) and Inter (everything else), bundled as static instances because
 * variable axes need API 26 and our floor is 24. Glyphs they lack (CJK, Arabic) fall back to the
 * system font.
 */
object MarqueeFonts {
    val display = FontFamily(Font(R.font.fraunces_semibold, FontWeight.SemiBold))
    val text =
        FontFamily(
            Font(R.font.inter_regular, FontWeight.Normal),
            Font(R.font.inter_medium, FontWeight.Medium),
            Font(R.font.inter_semibold, FontWeight.SemiBold),
        )
}

/** The steps of the type scale in `design/tokens.json`. */
enum class TextRole(
    internal val display: Boolean,
) {
    Hero(true),
    Title1(true),
    Title2(true),
    Title3(true),
    Headline(false),
    Body(false),
    Callout(false),
    Caption(false),
}

private fun TypeScale.size(role: TextRole): TextUnit =
    when (role) {
        TextRole.Hero -> hero
        TextRole.Title1 -> title1
        TextRole.Title2 -> title2
        TextRole.Title3 -> title3
        TextRole.Headline -> headline
        TextRole.Body -> body
        TextRole.Callout -> callout
        TextRole.Caption -> caption
    }

/** Text in the design system's scale; the size follows the form (TV points or phone sp). */
@Composable
fun MqText(
    text: String,
    modifier: Modifier = Modifier,
    role: TextRole = TextRole.Body,
    color: Color = Marquee.colors.text,
    weight: FontWeight? = null,
    maxLines: Int = Int.MAX_VALUE,
    textAlign: TextAlign? = null,
) {
    val size = Marquee.type.size(role)
    BasicText(
        text = text,
        modifier = modifier,
        style =
            TextStyle(
                color = color,
                fontSize = size,
                fontFamily = if (role.display) MarqueeFonts.display else MarqueeFonts.text,
                fontWeight = weight ?: if (role.display) FontWeight.SemiBold else FontWeight.Normal,
                lineHeight = size * (if (role.display) 1.1f else 1.4f),
                textAlign = textAlign ?: TextAlign.Unspecified,
            ),
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}
