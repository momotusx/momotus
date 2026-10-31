package app.marquee.ui.phone

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isHeading
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.StateThreadRule
import app.marquee.core.model.CastMember
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
import app.marquee.core.model.Trailer
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.route.Route
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import app.marquee.design.RemoteImage
import app.marquee.design.roundLabelsFit
import app.marquee.player.FULL_TRAILER_TAG
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
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertNull
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

    @get:Rule
    val stateThread = StateThreadRule()
    private val client = CoreClient(stateThread.dispatcher)
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
        SingletonImageLoader.reset()
    }

    /** Every picture asked for, in order. */
    private val asked = java.util.concurrent.CopyOnWriteArrayList<String>()

    /** Every picture answers (a small bitmap) but those in [missing], which fail as a 404 would. */
    private fun serve(vararg missing: String) {
        val picture = Bitmap.createBitmap(4, 6, Bitmap.Config.ARGB_8888).asImage()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    asked += data.toString()
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
    fun `part way in Start over sits beside Play, and Remove shows while the title is in Continue Watching`() {
        val resumed =
            title.copy(
                header =
                    title.header.copy(
                        playTarget = PlayTarget("tt1", "Resume", resumeMs = 60_000),
                        continuing = true,
                    ),
            )
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), resumed)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        val calls = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(
                    view,
                    null,
                    onPlay = { key, id -> calls += "play $key $id" },
                    onVersions = { calls += "versions $it" },
                    onEpisode = {},
                    onEpisodeVersions = {},
                    onUncontinue = { calls += "uncontinue" },
                    onStartOver = { key, id -> calls += "again $key $id" },
                )
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodes(hasContentDescription("Start over")).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNode(hasContentDescription("Start over")).performClick()
        compose.onNode(hasContentDescription("Remove from Continue Watching")).performClick()
        assertEquals(listOf("again null tt1", "uncontinue"), calls)
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

    /**
     * Draws [snapshot]'s page and waits for its Play; [before], if given, is drawn first until [beforeDone] says it is
     * done (a list's tile, as on the way to a title).
     */
    private fun draw(
        snapshot: TitleSnapshot,
        before: (@Composable () -> Unit)? = null,
        beforeDone: () -> Boolean = { true },
    ) {
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
        var page by mutableStateOf(before == null)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                if (page) {
                    PhoneTitle(
                        view,
                        versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false),
                        onPlay = { _, _ -> },
                        onVersions = {},
                        onEpisode = {},
                        onEpisodeVersions = {},
                    )
                } else {
                    before?.invoke()
                }
            }
        }
        compose.waitForIdle()
        if (!page) {
            compose.waitUntil(30_000) { beforeDone() }
            page = true
            compose.waitForIdle()
        }
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

    @Test
    fun `Trailer plays in the app full screen and never hands the watch page to another app`() {
        val trailer = Trailer("Sintel", "eRsGyueVLvQ", "https://www.youtube.com/watch?v=eRsGyueVLvQ")
        draw(title.copy(header = title.header.copy(details = title.header.details!!.copy(trailers = listOf(trailer)))))
        compose.onNode(hasContentDescription("Trailer").and(hasClickAction())).performClick()
        compose.waitForIdle()
        compose.onNodeWithTag(FULL_TRAILER_TAG).assertExists()
        assertNull(shadowOf(RuntimeEnvironment.getApplication()).nextStartedActivity)
        // Close takes it away, the page stays.
        compose.onNode(hasText("Close").and(hasClickAction())).performClick()
        compose.waitForIdle()
        compose.onAllNodesWithTag(FULL_TRAILER_TAG).assertCountEquals(0)
        compose.onNodeWithText("Play").assertExists()
    }

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
        // It prints its title at its top, so it starts under Back's row: Back never sits on it (R4).
        assertTrue(dp(art.top) >= BACK_ROW.value - 0.5f, "the cover starts at ${dp(art.top)} dp")
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
                            releaseInfo = "2001–2003",
                            runtime = "178 min",
                            ratings =
                                app.marquee.core.model
                                    .Ratings(imdb = "8.8"),
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

    // 800 dp high: the cover starts under Back's row (R4), so a 740 dp phone at this text size takes the band.
    @Test
    @Config(qualifiers = "w360dp-h800dp")
    fun `on a narrow phone with large text Play is whole on the first screen under the cover`() {
        RuntimeEnvironment.setFontScale(1.3f)
        draw(wordy)
        // The cover is shown (far taller than the band), and Play under it is whole in the window.
        val art = dp(bounds(TITLE_ART_TAG).height)
        assertTrue(art > band(360f, 740f) + 100f, "no cover: the art is $art dp")
        val (play, window) = playAndWindow()
        // Clear of a phone's navigation bar too (up to 48 dp, and the app's own bar over it): 56 dp kept free.
        assertTrue(play <= window - 56f, "Play ends at $play dp of $window: ${window - play} dp from the window's foot")
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
        // Clear of a phone's navigation bar too (up to 48 dp, and the app's own bar over it): 56 dp kept free.
        assertTrue(play <= window - 56f, "Play ends at $play dp of $window: ${window - play} dp from the window's foot")
    }

    /** Waits until the art is [height] dp high. */
    private fun artBecomes(height: Float) {
        val deadline = System.currentTimeMillis() + 30_000
        while (kotlin.math.abs(dp(bounds(TITLE_ART_TAG).height) - height) > 1f) {
            check(
                System.currentTimeMillis() < deadline,
            ) { "the art is ${dp(bounds(TITLE_ART_TAG).height)} dp, not $height" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(20)
        }
    }

    @Test
    fun `a large cover that does not load gives way to the addon's poster, still whole at 2 by 3`() {
        serve("https://a.example/c.jpg")
        draw(cover)
        compose.waitUntil(30_000) { "https://a.example/c.jpg" in asked }
        // The poster as the cover: the same 2:3 frame, no logo over it (it prints the title).
        artBecomes(411f * 1.5f)
        val deadline = System.currentTimeMillis() + 30_000
        while (asked.count { it == "https://a.example/p.jpg" } == 0) {
            check(System.currentTimeMillis() < deadline) { "the poster was never asked: $asked" }
            compose.waitForIdle()
            Thread.sleep(20)
        }
        compose.onAllNodesWithText("Sintel", useUnmergedTree = true).assertCountEquals(0)
    }

    @Test
    @Config(qualifiers = "w411dp-h740dp")
    fun `when the poster fails too the backdrop band and the logo come, and stay through a scroll`() {
        serve("https://a.example/c.jpg", "https://a.example/p.jpg")
        // A long page (cast, crew and the facts under the words), so the art leaves the list when it scrolls.
        val long =
            wordy.copy(
                header =
                    wordy.header.copy(
                        details =
                            wordy.header.details!!.copy(
                                cast = (1..8).map { CastMember("Actor Number $it") },
                                directors = listOf("A Director"),
                                writers = listOf("A Writer", "Another Writer"),
                            ),
                    ),
            )
        draw(long)
        artBecomes(band(411f, 740f))
        compose.onAllNodesWithText("Sintel", useUnmergedTree = true).assertCountEquals(1)
        // The art scrolled out of the list and back: the failed cover and poster are not asked again.
        val asks = asked.size
        val list = compose.onAllNodes(hasScrollAction()).onFirst()
        val count = list.fetchSemanticsNode().config[SemanticsProperties.CollectionInfo].rowCount
        list.performScrollToIndex(count - 1)
        compose.waitForIdle()
        assertTrue(
            compose.onAllNodesWithTag(TITLE_ART_TAG).fetchSemanticsNodes().isEmpty(),
            "the art never left the list",
        )
        list.performScrollToIndex(0)
        compose.waitForIdle()
        artBecomes(band(411f, 740f))
        assertEquals(asks, asked.size, "asked again: ${asked.drop(asks)}")
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

    /** Whether the circles' words fit a 360 dp phone's row (its side margins off) at [scale] times the text size. */
    private fun wordsFit(scale: Float): Boolean {
        RuntimeEnvironment.setFontScale(scale)
        var fit: Boolean? = null
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                fit =
                    roundLabelsFit(
                        listOf("Versions", "Trailer", "Add to Library", "Watched"),
                        360.dp - Marquee.safe.side * 2,
                    )
            }
        }
        compose.waitForIdle()
        return checkNotNull(fit)
    }

    @Test
    @Config(qualifiers = "w360dp-h740dp")
    // Real text measure: Robolectric's stand-in fonts are a few pixels a word.
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the circles keep their words at 1 3 times the text size on a 360 dp phone`() = assertTrue(wordsFit(1.3f))

    @Test
    @Config(qualifiers = "w360dp-h740dp")
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `at twice the text size the circles go without words, which would break mid word`() =
        assertTrue(!wordsFit(2.0f))

    @Test
    @Config(qualifiers = "w360dp-h700dp")
    fun `at twice the text size Play is whole on the first screen too`() {
        RuntimeEnvironment.setFontScale(2.0f)
        draw(wordy)
        val (play, window) = playAndWindow()
        // Clear of a phone's navigation bar too (up to 48 dp, and the app's own bar over it): 56 dp kept free.
        assertTrue(play <= window - 56f, "Play ends at $play dp of $window: ${window - play} dp from the window's foot")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the small poster paints from the list's memory under the large cover before its own fetch answers`() {
        // The poster is plain red the first time (the list's tile); after that, and the large cover, never answer.
        val red =
            Bitmap
                .createBitmap(
                    4,
                    6,
                    Bitmap.Config.ARGB_8888,
                ).apply { eraseColor(android.graphics.Color.RED) }
                .asImage()
        val posterAsks =
            java.util.concurrent.atomic
                .AtomicInteger()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    when (data.toString()) {
                        "https://a.example/c.jpg" -> awaitCancellation()
                        "https://a.example/p.jpg" -> if (posterAsks.getAndIncrement() > 0) awaitCancellation()
                    }
                    ImageFetchResult(red, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        val app = RuntimeEnvironment.getApplication()
        SingletonImageLoader.setUnsafe(ImageLoader.Builder(app).components { add(factory) }.build())
        val inMemory = {
            SingletonImageLoader
                .get(app)
                .memoryCache
                ?.keys
                ?.any { it.key.startsWith("https://a.example/p.jpg@") } ==
                true
        }
        draw(
            cover,
            before = { RemoteImage("https://a.example/p.jpg", 100.dp, 150.dp, "Sintel") },
            beforeDone = inMemory,
        )
        val art = bounds(TITLE_ART_TAG)
        val deadline = System.currentTimeMillis() + 20_000
        while (true) {
            val middle = compose.onRoot().captureToImage().toPixelMap()[art.center.x.toInt(), art.center.y.toInt()]
            if (middle.red > 0.8f && middle.green < 0.2f) break
            check(System.currentTimeMillis() < deadline) { "the cover's box shows $middle, not the poster from memory" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(50)
        }
        assertTrue(posterAsks.get() >= 2, "the poster's own fetch was asked: ${posterAsks.get()}")
    }

    @Test
    fun `a title with no art starts under the floating back link`() {
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
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = false, onBack = {}) {
                    PhoneTitle(
                        view,
                        null,
                        onPlay = { _, _ -> },
                        onVersions = {},
                        onEpisode = {},
                        onEpisodeVersions = {},
                    )
                }
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Sintel").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        val name = compose.onAllNodesWithText("Sintel").onFirst().getBoundsInRoot()
        // Over art, Back is the round button, spoken "Back".
        val back = compose.onNodeWithContentDescription("Back").getBoundsInRoot()
        assertTrue(name.top >= back.bottom, "the name starts at ${name.top}, the link ends at ${back.bottom}")
    }

    /**
     * R4 (Pixel walk 2026-10-30 F5): once the page scrolls, the status bar and Back's row take the page color, so what
     * scrolls up (Play, a cover's printed title) passes under the bar and never sits under Back. At rest there is no
     * bar: the art runs to the top under Back alone.
     */
    @Test
    @Config(qualifiers = "w411dp-h740dp")
    fun `once the page scrolls a bar of the page color holds Back, and at rest there is none`() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), wordy)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, statusStrip = false, onBack = {}) {
                    PhoneTitle(view, null, onPlay = {
                        _,
                        _,
                        ->
                    }, onVersions = {}, onEpisode = {}, onEpisodeVersions = {})
                }
            }
        }
        compose.waitForIdle()
        view.open()
        compose.waitUntil(120_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithTag(TITLE_ART_TAG).fetchSemanticsNodes().isNotEmpty()
        }

        fun bars() = compose.onAllNodesWithTag(TITLE_BAR_TAG).fetchSemanticsNodes()
        assertTrue(bars().isEmpty(), "a bar over the art at rest")
        // A short scroll, the art still under the status bar: the cover's top already reaches Back's row.
        compose
            .onAllNodes(hasScrollAction())
            .onFirst()
            .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, 40f * compose.density.density) }
        compose.waitForIdle()
        val bar = bars().singleOrNull()?.boundsInRoot ?: error("no bar after a scroll")
        val back = compose.onNodeWithContentDescription("Back").getBoundsInRoot()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        assertTrue(bar.left <= 0f && bar.right >= root.right, "the bar spans ${bar.left}..${bar.right}")
        assertTrue(
            bar.top <= 0f && dp(bar.bottom) >= back.bottom.value,
            "the bar ends at ${dp(bar.bottom)} dp, Back at ${back.bottom}",
        )
        compose
            .onAllNodes(hasScrollAction())
            .onFirst()
            .performSemanticsAction(SemanticsActions.ScrollBy) { it(0f, -400f * compose.density.density) }
        compose.waitForIdle()
        assertTrue(bars().isEmpty(), "the bar stayed back at the top")
    }
}
