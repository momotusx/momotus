package app.marquee.design

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.unit.Dp

/*
 * The app's mark (design/logo, plans/owner-picks.md): the owner's motmot, and the name in Unbounded SemiBold,
 * outlined, in mint. B (the bird before the name) where the name helps, C (the bird alone) where room is tight.
 */

/** C: the bird alone, `size` square. Not spoken: the place around it says what it is. */
@Composable
fun BrandMark(
    size: Dp,
    modifier: Modifier = Modifier,
) {
    Image(painterResource(R.drawable.brand_mark), contentDescription = null, modifier = modifier.size(size))
}

/**
 * B: the bird, then the name, its letters 0.45 of the bird's height. Spoken once, as the name, unless `spoken` is
 * false where text beside it already says the name.
 */
@Composable
fun BrandLockup(
    bird: Dp,
    modifier: Modifier = Modifier,
    spoken: Boolean = true,
) {
    Row(
        modifier.clearAndSetSemantics { if (spoken) contentDescription = "Momotus" },
        horizontalArrangement = Arrangement.spacedBy(bird * 0.2f),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BrandMark(bird)
        Image(
            painterResource(R.drawable.brand_wordmark),
            contentDescription = null,
            modifier = Modifier.height(bird * 0.45f),
        )
    }
}

/**
 * B while the app starts: a faint copy with the solid one revealed from the left, its edge feathered, over and
 * over, as the loading screen fills a title's logo (design/DECISION.md, Loading). Solid when animations are off.
 */
@Composable
fun BrandLoading(
    bird: Dp,
    modifier: Modifier = Modifier,
) {
    if (LocalReducedMotion.current) {
        BrandLockup(bird, modifier)
        return
    }
    val sweep =
        rememberInfiniteTransition(label = "brand").animateFloat(
            initialValue = -FEATHER,
            targetValue = 1f + FEATHER,
            animationSpec = infiniteRepeatable(tween(SWEEP_MS, easing = LinearEasing), RepeatMode.Restart),
            label = "sweep",
        )
    // Two copies drawn, one name spoken.
    Box(
        modifier.clearAndSetSemantics {
            contentDescription = "Momotus"
            testTag = BRAND_SWEEP
        },
    ) {
        BrandLockup(bird, Modifier.graphicsLayer { alpha = 0.2f })
        BrandLockup(
            bird,
            Modifier
                .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                .drawWithContent {
                    drawContent()
                    // Read in the draw phase: the sweep redraws this layer and recomposes nothing.
                    val p = sweep.value
                    drawRect(
                        Brush.horizontalGradient(
                            (p - FEATHER).coerceIn(0f, 1f) to Color.Black,
                            (p + FEATHER).coerceIn(0f, 1f) to Color.Transparent,
                        ),
                        blendMode = BlendMode.DstIn,
                    )
                },
        )
    }
}

/** The sweeping loading mark, for tests. */
const val BRAND_SWEEP = "brand-sweep"

private const val FEATHER = 0.03f
private const val SWEEP_MS = 2400
