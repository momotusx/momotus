package app.marquee.android

import android.app.Application
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import app.marquee.core.model.CatalogChoice
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.DiscoverState
import app.marquee.core.model.GenreChoice
import app.marquee.core.model.Platform
import app.marquee.core.model.Ratings
import app.marquee.core.model.SortChoice
import app.marquee.core.model.Tile
import app.marquee.core.model.Tuple2
import app.marquee.core.views.GenreChange
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.ui.tv.FocusMemory
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Discover's screen on snapshots made by hand (plans/discover-v2.md 6.1, 6.5): what its tests share. */
internal abstract class DiscoverScreenRules {
    @get:Rule
    val compose = createComposeRule()

    protected val picks = mutableListOf<DiscoverPick>()
    protected val focus = FocusMemory()
    protected var snapshot by mutableStateOf<DiscoverSnapshot?>(null)

    protected fun tile(id: String) =
        Tile(
            id = id,
            kind = "movie",
            name = "Title $id",
            posterShape = "poster",
            releaseInfo = "2020",
            ratings = Ratings(imdb = "7.1"),
        )

    protected fun discover(
        items: List<Tile> = (0 until 8).map { tile("t$it") },
        state: DiscoverState = DiscoverState.Ready,
        kinds: List<String> = listOf("movie", "series", "anime"),
        kind: String = "movie",
        genre: String? = null,
        platforms: List<Platform> = listOf(Platform("netflix", "Netflix"), Platform("paramount", "Paramount+")),
        platform: String? = null,
    ) = DiscoverSnapshot(
        kinds = kinds.map { Tuple2(it, NAMES.getValue(it)) },
        kind = kind,
        genreRequired = false,
        genre = genre,
        genreChoices = listOf(GenreChoice("Drama"), GenreChoice("Science Fiction")),
        decades = listOf("2020s", "2010s"),
        platforms = platforms,
        platform = platform,
        sorts =
            listOf(
                SortChoice("popular", "Popular"),
                SortChoice("imdb", "IMDb rating"),
                SortChoice("newest", "Newest"),
            ),
        sort = "imdb",
        lists = listOf(CatalogChoice("a|movie|top", "Top", "a")),
        items = items,
        state = state,
    )

    protected fun show(
        first: DiscoverSnapshot,
        form: Form = Form.Phone,
        fontScale: Float = 1f,
    ) {
        snapshot = first
        compose.setContent {
            val base = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(base.density, fontScale)) {
                MarqueeTheme(form) {
                    DiscoverContent(
                        snapshot,
                        failed = false,
                        pick = { picks += it },
                        loadMore = {},
                        retry = {},
                        onOpen = {},
                        onSetup = {},
                        onAddons = {},
                        focus = focus,
                    )
                }
            }
        }
        compose.waitForIdle()
    }

    protected fun tileNode(id: String) = compose.onNode(hasContentDescription("Title $id", substring = true))

    protected fun SemanticsNodeInteraction.right() =
        // Unclipped: a control scrolled out of a row is clipped away from its bounds in the root.
        fetchSemanticsNode().let { it.positionInRoot.x + it.size.width }

    protected fun SemanticsNodeInteraction.lines(): Int {
        val layouts = mutableListOf<TextLayoutResult>()
        fetchSemanticsNode().config[SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
        return layouts.single().lineCount
    }

    protected companion object {
        val NAMES =
            mapOf(
                "movie" to "Movies",
                "series" to "Series",
                "anime" to "Anime",
                "channel" to "Channels",
                "tv" to "Live TV",
            )
    }
}

/** The behaviour, on a 360 dp phone and on TV. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w360dp-h800dp")
internal class DiscoverScreenTest : DiscoverScreenRules() {
    @Test
    fun `Clear shows only with a filter, and clears the genre, the service and the year in one select`() {
        show(discover())
        compose.onAllNodesWithText("Clear").assertCountEquals(0)
        snapshot = discover(genre = "Drama", platform = "netflix")
        compose.onNodeWithContentDescription("Clear filters").performClick()
        assertEquals(
            listOf(DiscoverPick(genre = GenreChange.Clear, platform = GenreChange.Clear, decade = GenreChange.Clear)),
            picks,
        )
    }

    @Test
    fun `More lists only the other kinds, and the Sort menu heads the other catalogs`() {
        show(discover(kinds = listOf("movie", "series", "anime", "channel", "tv")))
        compose.onNodeWithContentDescription("More").performClick()
        // The menu's title alone (the button says it as its label): no "More" row that does nothing.
        compose.onAllNodesWithText("More").assertCountEquals(1)
        compose.onNodeWithText("Live TV").performClick()
        compose.onNodeWithContentDescription("Sort, IMDb rating").performClick()
        compose.onNodeWithText("More lists").assertExists()
        assertEquals(DiscoverPick(kind = "tv"), picks.single())
    }

    @Test
    fun `a list loading after a change keeps the one before, dimmed and out of reach`() {
        show(discover())
        snapshot = discover(items = emptyList(), state = DiscoverState.Loading)
        compose.waitForIdle()
        tileNode("t0").assertIsNotEnabled()
    }

    @Test
    fun `a title arriving ahead keeps the tile under the thumb where it was`() {
        val items = (0 until 40).map { tile("t$it") }
        show(discover(items = items))
        compose
            .onNode(
                hasScrollToNodeAction() and SemanticsMatcher.keyIsDefined(SemanticsProperties.VerticalScrollAxisRange),
            ).performScrollToNode(hasContentDescription("Title t20", substring = true))
        val before = tileNode("t20").fetchSemanticsNode().boundsInRoot
        // Whole lines of titles ahead (six: two or three columns), so the tile keeps its column too.
        snapshot = discover(items = (0 until 6).map { tile("x$it") } + items)
        compose.waitForIdle()
        assertEquals(before, tileNode("t20").fetchSemanticsNode().boundsInRoot)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp")
    fun `on TV the first entry lands on the selected kind`() {
        show(discover(kind = "series"), Form.Tv)
        compose.runOnIdle { focus.requester(DISCOVER_KIND_ENTRY)!!.requestFocus() }
        compose.onNodeWithText("Series").assertIsFocused()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp")
    fun `on TV Up from the grid lands on the filter control focused last`() {
        show(discover(), Form.Tv)
        compose.onNodeWithContentDescription("Sort, IMDb rating").requestFocus()
        tileNode("t0").requestFocus()
        tileNode("t0").performKeyInput { pressKey(Key.DirectionUp) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Sort, IMDb rating").assertIsFocused()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp")
    fun `on TV Clear hands focus to the genre menu`() {
        show(discover(genre = "Drama"), Form.Tv)
        compose.onNodeWithContentDescription("Clear filters").requestFocus()
        compose.onNodeWithContentDescription("Clear filters").performKeyInput { pressKey(Key.DirectionCenter) }
        snapshot = discover()
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Genre, Any genre").assertIsFocused()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp")
    fun `on TV a late list that reorders the grid waits while focus is in it`() {
        val items = (0 until 6).map { tile("t$it") }
        show(discover(items = items), Form.Tv)
        tileNode("t0").requestFocus()
        snapshot = discover(items = listOf(tile("x")) + items)
        compose.waitForIdle()
        tileNode("x").assertDoesNotExist()
        compose.onNodeWithText("More titles arrived. Show").performClick()
        tileNode("x").assertExists()
    }
}

/** The layout on a 360 dp phone, with real fonts (native graphics), since widths and lines are what it is about. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], application = Application::class, qualifiers = "w360dp-h800dp")
internal class DiscoverLayoutTest : DiscoverScreenRules() {
    @Test
    fun `at 360 dp and font scale 1_3 every control is on the screen`() {
        show(
            discover(
                kinds = listOf("movie", "series", "anime", "channel"),
                genre = "Science Fiction",
                platform = "netflix",
            ),
            fontScale = 1.3f,
        )
        val width =
            compose
                .onRoot()
                .fetchSemanticsNode()
                .size.width
        val controls =
            listOf(
                compose.onNodeWithText("Movies"),
                compose.onNodeWithText("Anime"),
                compose.onNodeWithContentDescription("More"),
                compose.onNodeWithContentDescription("Genre, Science Fiction"),
                compose.onNodeWithContentDescription("Year, Any year"),
                compose.onNodeWithContentDescription("Clear filters"),
                compose.onNodeWithContentDescription("Sort, IMDb rating"),
            )
        for (control in controls) assertTrue(control.right() <= width, "${control.fetchSemanticsNode().config} is cut")
    }

    @Test
    fun `the rail shows only with services, and a service with no logo is its name on one line`() {
        show(discover(platforms = emptyList()))
        compose.onAllNodesWithText("All", useUnmergedTree = true).assertCountEquals(0)
        snapshot = discover()
        compose.waitForIdle()
        assertEquals(1, compose.onNodeWithText("Paramount+", useUnmergedTree = true).lines())
        // As wide as its name, not a square that breaks it: wider than All's tile.
        val width = { name: String ->
            compose
                .onNodeWithContentDescription(name)
                .fetchSemanticsNode()
                .size.width
        }
        assertTrue(width("Paramount+") > width("All"), "${width("Paramount+")} vs ${width("All")}")
    }
}
