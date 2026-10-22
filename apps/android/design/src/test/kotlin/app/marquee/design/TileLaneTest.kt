package app.marquee.design

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.Dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * A row's height comes from the tile tokens (the caption room, `captionPoster` for posters), the same on web
 * and Apple, and a tile's two caption lines, measured with real text (a 16:9 card's scores after its line), fit
 * inside that room.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w1920dp-h1080dp")
class TileLaneTest {
    @get:Rule
    val compose = createComposeRule()

    private fun lane(
        form: Form,
        shape: TileShape,
    ) {
        var lane = Dp.Unspecified
        var room = Dp.Unspecified
        compose.setContent {
            MarqueeTheme(form) {
                val metrics = Marquee.tiles
                lane = metrics.laneHeight(shape)
                room = if (shape == TileShape.Poster) metrics.captionPoster else metrics.caption
                PosterTile(
                    "A long title that runs past the tile",
                    null,
                    {},
                    shape = shape,
                    caption = "2010",
                    ratings = listOf("IMDb 10.0"),
                )
            }
        }
        val metrics = if (form == Form.Tv) Tokens.tvTiles else Tokens.phoneTiles
        assertEquals(
            metrics.pad * 2 + metrics.heightOf(shape) + room,
            lane,
            "$form $shape: the lane is not the tokens'",
        )
        val art = compose.onNodeWithContentDescription("A long title that runs past the tile").fetchSemanticsNode()
        val year = compose.onNodeWithText("2010", useUnmergedTree = true).fetchSemanticsNode()
        val used = (year.boundsInRoot.bottom - art.boundsInRoot.bottom) / compose.density.density
        assertTrue(used <= room.value + 0.5f, "$form $shape: the caption takes $used dp of ${room.value}")
    }

    @Test
    fun `a TV poster row is the tokens' height and its caption fits`() = lane(Form.Tv, TileShape.Poster)

    @Test
    fun `a TV 16 to 9 row is the tokens' height and its caption fits`() = lane(Form.Tv, TileShape.Landscape)

    @Test
    fun `a phone poster row is the tokens' height and its caption fits`() = lane(Form.Phone, TileShape.Poster)

    @Test
    fun `a phone 16 to 9 row is the tokens' height and its caption fits`() = lane(Form.Phone, TileShape.Landscape)
}
