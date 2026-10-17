package app.marquee.design

import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.em

/**
 * Inter everywhere (design/DECISION.md v2): text at opsz 14 in four weights, and display titles
 * and headings at opsz 32 in 800. Static instances, because variable axes need API 26 and our
 * floor is 24. Glyphs Inter lacks (CJK, Arabic) fall back to the system font.
 */
object MarqueeFonts {
    val display = FontFamily(Font(R.font.inter_display_extrabold, FontWeight(Tokens.weight.display)))
    val text =
        FontFamily(
            Font(R.font.inter_regular, FontWeight.Normal),
            Font(R.font.inter_medium, FontWeight.Medium),
            Font(R.font.inter_semibold, FontWeight.SemiBold),
            Font(R.font.inter_bold, FontWeight.Bold),
        )
}

/** The steps of the type scale in `design/tokens.json`; [tracking] is set for display and heading roles. */
enum class TextRole(
    internal val tracking: Float?,
) {
    Hero(Tokens.tracking.display),
    Title1(Tokens.tracking.display),
    Title2(Tokens.tracking.heading),
    Title3(Tokens.tracking.heading),
    Headline(null),
    Body(null),
    Callout(null),
    Caption(null),
    ;

    internal val display: Boolean get() = tracking != null
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
    /** Told after layout whether [maxLines] cut the text, so a screen can offer "More". */
    onTextLayout: ((TextLayoutResult) -> Unit)? = null,
    /** Overrides the role's tracking (spaced capitals of an eyebrow). */
    letterSpacing: TextUnit = TextUnit.Unspecified,
) {
    val size = Marquee.type.size(role)
    BasicText(
        text = text,
        modifier = modifier,
        onTextLayout = onTextLayout,
        style =
            TextStyle(
                color = color,
                fontSize = size,
                fontFamily = if (role.display) MarqueeFonts.display else MarqueeFonts.text,
                fontWeight = weight ?: if (role.display) FontWeight(Tokens.weight.display) else FontWeight.Normal,
                letterSpacing =
                    if (letterSpacing !=
                        TextUnit.Unspecified
                    ) {
                        letterSpacing
                    } else {
                        role.tracking?.em ?: TextUnit.Unspecified
                    },
                lineHeight = size * (if (role.display) 1.1f else 1.4f),
                textAlign = textAlign ?: TextAlign.Unspecified,
            ),
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}
