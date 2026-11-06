package app.marquee.design

import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.createFontFamilyResolver
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.sp
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals

/** A tile's caption line costs one text measure, whatever its length (plans/batch-13-perf.md, android-perf). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class FitLineTest {
    private val measurer =
        TextMeasurer(createFontFamilyResolver(RuntimeEnvironment.getApplication()), Density(1f), LayoutDirection.Ltr)
    private val style = TextStyle(fontSize = 14.sp)

    private fun measure(text: String) = measurer.measure(text, style, maxLines = 1, softWrap = false)

    private val titles =
        listOf(
            "The Lord of the Rings: The Return of the King",
            "Dune",
            "S2:E3 · 36 min left",
            "Supercalifragilisticexpialidocious film",
        )

    @Test
    fun `a line is measured once, and cut where measuring every cut would cut it`() {
        for (room in listOf(40, 80, 120, 160, 400)) {
            for (title in titles) {
                var measures = 0
                val shown =
                    fitLine(title, room) {
                        measures++
                        measure(it)
                    }
                assertEquals(1, measures, "$title in $room px")
                assertEquals(wordFit(title) { measure(it).size.width <= room }, shown, "$title in $room px")
            }
        }
    }
}
