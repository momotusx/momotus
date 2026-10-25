package app.marquee.design

import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The owner's C2 (plans/appletv-polish.md 4.3) and B2: a Top 10 place over the cover's top left, its genre under the art. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w411dp-h891dp")
class RankTileTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun aTop10PlaceSitsAtTheArtsTopLeftAndItsGenreUnderTheArt() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PosterTile("Sintel", null, {}, captioned = false, rank = 3, genre = "Drama")
            }
        }
        val poster = Tokens.phoneTiles.poster
        val rank = compose.onNodeWithText("3", useUnmergedTree = true).getUnclippedBoundsInRoot()
        assertTrue(
            rank.left < poster.width / 2 && rank.top < poster.height / 4,
            "the place at the art's top left: $rank",
        )
        val genre = compose.onNodeWithTag(RANK_GENRE_TAG, useUnmergedTree = true).getUnclippedBoundsInRoot()
        assertTrue(genre.top >= poster.height, "the genre under the art, off the cover's band: $genre")
    }

    @Test
    fun theOwnersB2Timings() {
        assertEquals(8000L, HeroClock.DWELL_MS)
        assertEquals(650, HeroClock.SLIDE_MS)
        assertEquals(0.85f, Tokens.HERO_PARALLAX)
        assertTrue(0.dp < Tokens.phoneTiles.poster.width)
    }
}
