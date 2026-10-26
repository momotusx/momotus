package app.marquee.ui.phone

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isHeading
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.Network
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.Speed
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.Uri
import coil3.asImage
import coil3.decode.DataSource
import coil3.fetch.FetchResult
import coil3.fetch.Fetcher
import coil3.fetch.ImageFetchResult
import coil3.request.Options
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone title page's button when nothing can play (plans/play-every-version.md, the shared table): each new
 * action does what it says when pressed, not only in TitleCopy.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h2400dp")
class PhoneTitleActionsTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
        SingletonImageLoader.reset()
    }

    /** Every picture answers (a small bitmap) but those in [missing], which fail as a 404 would. */
    private fun serve(vararg missing: String) {
        val picture = Bitmap.createBitmap(4, 6, Bitmap.Config.ARGB_8888).asImage()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    if (data.toString() in missing) error("HTTP 404")
                    ImageFetchResult(picture, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
    }

    @Before
    fun pictures() = serve()

    private val title =
        TitleSnapshot(
            TitleHeader(
                state = TitleState.Ready,
                details = TitleDetails(id = "tt1", kind = "movie", name = "Sintel", source = "local"),
                inLibrary = false,
                playTarget = PlayTarget("tt1", "Play"),
                watched = false,
            ),
        )

    private fun versions(
        kind: EmptyKind,
        action: EmptyAction,
        key: String? = null,
    ) = VersionsSnapshot(
        conditions =
            VersionsConditions(
                screenClass = 1080,
                hdr = false,
                network = Network.Wifi,
                http = Speed(42.2f, 3),
                p2p = Speed(10f, 0),
                minSeeders = 50,
                engine = false,
                account = true,
            ),
        empty = EmptyVersions(kind, action, 0, key = key),
        hidden = HiddenSummary(0),
        done = true,
        frozen = false,
        nobodySharing = false,
    )

    @Test
    fun `Play is the one primary - it plays the kept version, opens Versions, or is inert`() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), title)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        var shown by mutableStateOf(versions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, key = "kept"))
        val calls = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(
                    view,
                    shown,
                    onPlay = { key, id -> calls += "play $key $id" },
                    onVersions = { calls += "versions $it" },
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Play").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        // An owner's page offers Add to Library (the core's librarySettable).
        compose.onAllNodes(hasContentDescription("Add to Library")).assertCountEquals(1)
        val play = { compose.onNodeWithText("Play").performClick() }
        play()
        assertEquals(listOf("play kept tt1"), calls)
        // Still searching: the player opens at once, with no version yet; its pick starts when it arrives.
        shown = versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false)
        compose.waitForIdle()
        calls.clear()
        play()
        assertEquals(listOf("play null tt1"), calls)
        // No Settings or account step on the play surface: Play opens Versions, which holds them.
        for ((kind, action) in listOf(
            EmptyKind.NoSource to EmptyAction.SettingsAddons,
            EmptyKind.Unreachable to EmptyAction.TryAgain,
            EmptyKind.NothingPlaysHere to EmptyAction.ChooseVersion,
        )) {
            shown = versions(kind, action)
            compose.waitForIdle()
            for (gone in listOf("Settings", "Try again", "Play from this device", "Choose a version")) {
                compose.onAllNodesWithText(gone).assertCountEquals(0)
            }
            calls.clear()
            play()
            assertEquals(listOf("versions tt1"), calls, "$kind")
        }
        // A state whose only way on is Back: Play keeps its place, inert, with the state's line under it.
        shown = versions(EmptyKind.NoResults, EmptyAction.None)
        compose.waitForIdle()
        compose.onNodeWithText("No source has this yet", substring = true, useUnmergedTree = true).assertExists()
        compose.onNodeWithText("Play").assertIsNotEnabled()
        calls.clear()
        play()
        assertEquals(emptyList(), calls)
        // Versions that need an account: the quiet line, Play inert, and no account step anywhere on the page
        // (accounts are set up in Settings only).
        shown = versions(EmptyKind.NeedsAccount, EmptyAction.None)
        compose.waitForIdle()
        compose
            .onNodeWithText(
                "None of these plays on this device",
                substring = true,
                useUnmergedTree = true,
            ).assertExists()
        for (gone in listOf("Add an account", "Settings", "streaming account")) {
            compose.onAllNodesWithText(gone, substring = true, useUnmergedTree = true).assertCountEquals(0)
        }
        compose.onNodeWithText("Play").assertIsNotEnabled()
        calls.clear()
        play()
        assertEquals(emptyList(), calls)
    }

    @Test
    fun `a kids profile's page shows no Add to Library`() {
        val kids = title.copy(header = title.header.copy(librarySettable = false))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), kids)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(
                    view,
                    versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false),
                    onPlay = { _, _ -> },
                    onVersions = {},
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Play").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onAllNodes(hasContentDescription("Add to Library")).assertCountEquals(0)
        compose.onAllNodes(hasContentDescription("In Library")).assertCountEquals(0)
    }

    /** Draws [snapshot]'s page and waits for its Play. */
    private fun draw(snapshot: TitleSnapshot) {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(
                    view,
                    versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false),
                    onPlay = { _, _ -> },
                    onVersions = {},
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Play").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot

    private fun withArt(
        poster: String? = null,
        cover: String? = null,
        background: String? = null,
        titled: Boolean = cover != null,
    ) = title.copy(
        header =
            title.header.copy(
                details =
                    title.header.details!!.copy(
                        poster = poster,
                        cover = cover,
                        coverTitled = titled,
                        background = background,
                    ),
            ),
    )

    private val cover =
        withArt(
            poster = "https://a.example/p.jpg",
            cover = "https://a.example/c.jpg",
            background = "https://a.example/b.jpg",
        )

    /** The backdrop band's height in this window (`heroHeight` without a cover). */
    private fun band(
        width: Float,
        height: Float,
    ) = minOf(width / 1.45f, height * 0.6f)

    private fun dp(px: Float) = px / compose.density.density

    @Test
    fun `a cover shows whole in a 2 by 3 frame`() {
        draw(cover)
        val art = bounds(TITLE_ART_TAG)
        // 411 dp wide: 616.5 dp high, far under 75 percent of the 2400 dp window.
        assertEquals(1.5f, art.height / art.width, 0.01f)
    }

    @Test
    @Config(qualifiers = "w800dp-h360dp")
    fun `a landscape phone keeps the backdrop band, not a strip of the cover`() {
        draw(cover)
        assertEquals(band(800f, 360f), dp(bounds(TITLE_ART_TAG).height), 1f)
        compose.onAllNodesWithText("Sintel", useUnmergedTree = true).assertCountEquals(1)
    }

    @Test
    @Config(qualifiers = "w800dp-h1280dp")
    fun `a tablet keeps the backdrop band`() {
        draw(cover)
        assertEquals(band(800f, 1280f), dp(bounds(TITLE_ART_TAG).height), 1f)
    }

    @Test
    @Config(qualifiers = "w1280dp-h800dp")
    fun `a landscape tablet keeps the backdrop band`() {
        draw(cover)
        assertEquals(band(1280f, 800f), dp(bounds(TITLE_ART_TAG).height), 1f)
    }

    /** The cover with a full page of words: a long synopsis, three genres and a year and runtime. */
    private val wordy =
        cover.copy(
            header =
                cover.header.copy(
                    details =
                        cover.header.details!!.copy(
                            description = "A long synopsis that runs on. ".repeat(20),
                            genres = listOf("Animation", "Fantasy", "Adventure"),
                            releaseInfo = "2010",
                            runtime = "15 min",
                        ),
                ),
        )

    /** Play's bottom and the window's, in dp. */
    private fun playAndWindow(): Pair<Float, Float> {
        val play = compose.onNodeWithText("Play").fetchSemanticsNode().boundsInRoot
        return dp(play.bottom) to
            dp(
                compose
                    .onRoot()
                    .fetchSemanticsNode()
                    .boundsInRoot.bottom,
            )
    }

    @Test
    @Config(qualifiers = "w360dp-h740dp")
    fun `on a narrow phone with large text Play is whole on the first screen under the cover`() {
        RuntimeEnvironment.setFontScale(1.3f)
        draw(wordy)
        // The cover is shown (far taller than the band), and Play under it is whole in the window.
        val art = dp(bounds(TITLE_ART_TAG).height)
        assertTrue(art > band(360f, 740f) + 100f, "no cover: the art is $art dp")
        val (play, window) = playAndWindow()
        assertTrue(play <= window, "Play ends at $play dp of $window")
        // Play comes before the synopsis and the genres, right under the meta line, as on the iPhone (where they
        // sit, clipped or not).
        val words =
            listOf("A long synopsis", "Fantasy").flatMap { text ->
                compose
                    .onAllNodesWithText(text, substring = true, useUnmergedTree = true)
                    .fetchSemanticsNodes()
                    .map { dp(it.positionInRoot.y) }
            }
        assertTrue(words.size >= 2, "the synopsis and the genres: $words")
        for (top in words) assertTrue(play <= top, "Play ends at $play dp, over words at $top")
    }

    @Test
    @Config(qualifiers = "w360dp-h640dp")
    fun `on a small phone with large text Play is whole on the first screen`() {
        RuntimeEnvironment.setFontScale(1.3f)
        draw(wordy)
        val (play, window) = playAndWindow()
        assertTrue(play <= window, "Play ends at $play dp of $window")
    }

    @Test
    fun `a cover that does not load gives way to the backdrop band and the logo`() {
        serve("https://a.example/c.jpg")
        draw(cover)
        val deadline = System.currentTimeMillis() + 30_000
        while (dp(bounds(TITLE_ART_TAG).height) > band(411f, 2400f) + 1f) {
            check(System.currentTimeMillis() < deadline) { "the cover never gave way" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(20)
        }
        compose.onAllNodesWithText("Sintel", useUnmergedTree = true).assertCountEquals(1)
    }

    @Test
    fun `a cover not known to print its title keeps the name under it`() {
        draw(withArt(poster = "https://a.example/p.jpg", cover = "https://a.example/c.jpg", titled = false))
        val art = bounds(TITLE_ART_TAG)
        val name = compose.onNodeWithText("Sintel", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue(name.top >= art.bottom, "the name at ${name.top}, the cover's foot at ${art.bottom}")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a cover carries no second title and its bottom edge is the page color`() {
        draw(withArt(poster = "https://a.example/p.jpg", cover = "https://a.example/c.jpg"))
        // The cover prints its own title: no logo or name drawn over it, only the heading for screen readers.
        compose.onAllNodesWithText("Sintel", useUnmergedTree = true).assertCountEquals(0)
        compose.onNode(hasContentDescription("Sintel") and isHeading()).assertExists()
        // The art's last row of pixels is the page's color, as just under it, where the meta line starts.
        val art = bounds(TITLE_ART_TAG)
        val shot = compose.onRoot().captureToImage().toPixelMap()
        val under = shot[shot.width / 2, art.bottom.toInt() + 2]
        for (x in 0 until shot.width step 16) {
            val edge = shot[x, art.bottom.toInt() - 1]
            for ((a, b) in listOf(edge.red to under.red, edge.green to under.green, edge.blue to under.blue)) {
                assertEquals(b, a, 2f / 255, "x $x: $edge against the page's $under")
            }
        }
    }

    @Test
    fun `a backdrop alone keeps the wide frame`() {
        draw(withArt(background = "https://a.example/b.jpg"))
        val art = bounds(TITLE_ART_TAG)
        assertEquals(1 / 1.45f, art.height / art.width, 0.01f)
    }

    @Test
    fun `Watched is a small circle in the actions' row, never a big button`() {
        draw(title)
        compose.onAllNodesWithText("Mark as watched").assertCountEquals(0)
        // One name, a switch's state (off here); its circle is Library's, level with it.
        val node = compose.onNode(hasContentDescription("Watched") and hasClickAction())
        node.assert(SemanticsMatcher.expectValue(SemanticsProperties.ToggleableState, ToggleableState.Off))
        node.assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Switch))
        val watched = node.fetchSemanticsNode().boundsInRoot
        val library = compose.onNode(hasContentDescription("Add to Library")).fetchSemanticsNode().boundsInRoot
        assertEquals(library.top, watched.top, 1f)
        assertEquals(library.width, watched.width, 1f)
        assertEquals(library.height, watched.height, 1f)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the small poster stands under the large cover until it lands`() {
        // The small poster is plain red; the large cover never answers.
        val red =
            Bitmap
                .createBitmap(
                    4,
                    6,
                    Bitmap.Config.ARGB_8888,
                ).apply { eraseColor(android.graphics.Color.RED) }
                .asImage()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    if (data.toString() == "https://a.example/c.jpg") awaitCancellation()
                    ImageFetchResult(red, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
        draw(cover)
        val art = bounds(TITLE_ART_TAG)
        val deadline = System.currentTimeMillis() + 20_000
        while (true) {
            val middle = compose.onRoot().captureToImage().toPixelMap()[art.center.x.toInt(), art.center.y.toInt()]
            if (middle.red > 0.8f && middle.green < 0.2f) break
            check(System.currentTimeMillis() < deadline) { "the cover's box shows $middle, not the small poster" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(50)
        }
    }
}
