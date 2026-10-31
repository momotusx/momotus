package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.pressKey
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeRow
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.MarqueeTheme
import app.marquee.design.SECONDARY_FILL
import app.marquee.design.ScrimStop
import app.marquee.design.Scrims
import app.marquee.design.Tokens
import app.marquee.design.formPalette
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.Uri
import coil3.asImage
import coil3.decode.DataSource
import coil3.fetch.FetchResult
import coil3.fetch.Fetcher
import coil3.fetch.ImageFetchResult
import coil3.request.Options
import kotlinx.coroutines.awaitCancellation
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/**
 * Settings A1 (design/DECISION.md): the header's film, its one height, the kids rule, the TV menu and its focus, the
 * centered column, and every word on the header over white art. With MQ_SHOTS set, the render tests save theirs there.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SettingsA1Test {
    @get:Rule
    val compose = createComposeRule()

    @After
    fun reset() = SingletonImageLoader.reset()

    private val me =
        ProfileInfo("p1", "Mia", "letter:sky", 0, hasPin = false, isOwner = true, syncing = false, kids = false)

    private fun tile(
        id: String,
        background: String?,
        logo: String? = null,
    ) = Tile(id = id, kind = "movie", name = id, background = background, logo = logo, posterShape = "poster")

    /** While true, a picture never lands (the art on its way). */
    @Volatile private var holding = false

    /** Every picture is [picture]: one color, or a sky over hills for the shots. */
    private fun serve(picture: Bitmap = solid(android.graphics.Color.WHITE)) {
        val image = picture.asImage()
        val factory =
            Fetcher.Factory<Uri> { _, _: Options, _: ImageLoader ->
                Fetcher {
                    if (holding) awaitCancellation()
                    ImageFetchResult(image, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        val loader = ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build()
        SingletonImageLoader.setUnsafe(loader)
    }

    private fun solid(color: Int) = Bitmap.createBitmap(48, 20, Bitmap.Config.ARGB_8888).apply { eraseColor(color) }

    private fun scene(): Bitmap {
        val b = Bitmap.createBitmap(960, 402, Bitmap.Config.ARGB_8888)
        val c = Canvas(b)
        val gradient = LinearGradient(0f, 0f, 0f, 260f, 0xFF7FA7D9.toInt(), 0xFFE9D3B0.toInt(), Shader.TileMode.CLAMP)
        val sky = Paint().apply { shader = gradient }
        c.drawRect(0f, 0f, 960f, 402f, sky)
        c.drawCircle(220f, 520f, 300f, Paint().apply { color = 0xFF3F6B3A.toInt() })
        c.drawCircle(780f, 560f, 340f, Paint().apply { color = 0xFF2E5530.toInt() })
        return b
    }

    /** The page's groups as Settings lays them out, with its real words. */
    private fun parts(): List<SettingsPart> =
        listOf(
            SettingsPart(SettingsSection.Sources) {
                SettingsGroup(
                    {
                        SettingsRow(
                            Icons.Sparkles,
                            Tone.Green,
                            "Recommended addons",
                            "Catalogs, streams and lists.",
                            {},
                        )
                    },
                    {
                        SettingsRow(
                            Icons.PackageSearch,
                            Tone.Teal,
                            "Find addons",
                            "Most add subtitles and catalogs.",
                            {},
                        )
                    },
                    { SettingsRow(Icons.Puzzle, Tone.Mint, "Addons", "4 installed. 2 find streams.", {}) },
                    {
                        SettingsRow(
                            Icons.KeyRound,
                            Tone.Sky,
                            "Debrid services",
                            "Play torrents as fast direct links.",
                            {},
                        )
                    },
                    { SettingsRow(Icons.Radio, Tone.Blue, "Trakt", "Not connected.", {}) },
                    {
                        SettingsRow(
                            Icons.Download,
                            Tone.Sea,
                            "Import from Stremio",
                            "Addons, library and progress, with a code.",
                            {},
                        )
                    },
                )
            },
            SettingsPart(SettingsSection.Account) {
                SettingsGroup({
                    SettingsRow(
                        Icons.MonitorSmartphone,
                        Tone.Sky,
                        "Account",
                        "This device only. Turn on sync to follow you.",
                        {},
                    )
                })
            },
            SettingsPart(SettingsSection.Home) {
                SettingsGroup(
                    {
                        SettingsRow(
                            Icons.Rows3,
                            Tone.Mint,
                            "Home rows",
                            "Which catalogs Home shows, and in what order.",
                            {},
                        )
                    },
                )
            },
            SettingsPart(SettingsSection.Playback) {
                SettingsGroup({ SettingsRow(Icons.SlidersHorizontal, Tone.Teal, "Stream choice", "Start fast.", {}) })
            },
            SettingsPart(SettingsSection.Subtitles) {
                SettingsGroup({ SettingsRow(Icons.Captions, Tone.Sky, "Second line", "Spoken language.", {}) })
            },
            SettingsPart(SettingsSection.About) {
                SettingsGroup({ SettingsRow(Icons.Info, Tone.Grey, "Momotus", "Free software under the GPL 3.", {}) })
            },
        )

    private fun phone(
        art: () -> Tile?,
        who: ProfileInfo? = me,
    ) = compose.setContent {
        MarqueeTheme(Form.Phone) { PhoneSettings(who, profileLine(false), art(), {}, parts()) }
    }

    private fun tv(
        art: () -> Tile?,
        canEdit: Boolean = true,
    ) = compose.setContent {
        MarqueeTheme(Form.Tv) { TvSettings(me, profileLine(false), art(), canEdit, canEdit, {}, {}, {}, parts()) }
    }

    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot

    private fun boundsOf(node: SemanticsNodeInteraction): Rect = node.fetchSemanticsNode().boundsInRoot

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    /** Waits until the art has landed and faded in: its top trailing corner is no longer the page. */
    private fun artLanded() {
        compose.waitUntil(5_000) {
            val a = bounds(SETTINGS_ART)
            val px = compose.onRoot().captureToImage().toPixelMap()[a.right.toInt() - 10, maxOf(a.top.toInt(), 0) + 10]
            px != Color.Black && px != formPalette(Form.Tv).bg
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.waitForIdle()
    }

    private fun focused() = compose.onNode(isFocused())

    private val inMenu = hasAnyAncestor(hasTestTag(SETTINGS_MENU))
    private val inList = hasAnyAncestor(hasTestTag(SETTINGS_LIST))

    @Test
    fun `the film is Continue Watching's first with a backdrop, else the hero's, a kids profile takes its own only`() {
        val bare = tile("bare", null)
        val watching = tile("watching", "https://art.example/cw.jpg")
        val hero = Hero(tile("hero", "https://art.example/hero.jpg"), reason = "Popular")
        val cw = HomeRow("continue-watching", "Continue Watching", "", RowState.Ready, listOf(bare, watching))
        val other =
            HomeRow("a:movie:top", "Popular", "", RowState.Ready, listOf(tile("other", "https://art.example/o.jpg")))
        assertSame(watching, settingsArt(listOf(other, cw), listOf(hero), kids = false))
        assertSame(hero.tile, settingsArt(listOf(other, cw.copy(tiles = listOf(bare))), listOf(hero), kids = false))
        assertSame(hero.tile, settingsArt(emptyList(), listOf(Hero(bare, reason = ""), hero), kids = false))
        assertSame(watching, settingsArt(listOf(cw), listOf(hero), kids = true))
        assertNull(settingsArt(emptyList(), listOf(hero), kids = true))
    }

    @Test
    fun `a kids profile has no Edit profile and no Manage profiles`() {
        assertSame(me, editableProfile(me, kids = false))
        assertNull(editableProfile(me.copy(kids = true), kids = false))
        assertNull(editableProfile(me, kids = true))
        assertNull(editableProfile(null, kids = false))
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, a profile that cannot edit shows neither button`() {
        serve()
        tv({ null }, canEdit = false)
        compose.waitForIdle()
        compose.onNodeWithText("Edit profile").assertDoesNotExist()
        compose.onNodeWithText("Manage profiles").assertDoesNotExist()
        compose.onNodeWithText("Mia").assertExists()
    }

    /** The header's height with art landed, before it lands, and with none: one height from the first frame. */
    private fun oneHeight(
        expected: Float,
        show: (() -> Tile?) -> Unit,
    ) {
        serve()
        var art by mutableStateOf<Tile?>(null)
        show { art }
        compose.waitForIdle()
        val none = bounds(SETTINGS_HEADER).height
        holding = true
        art = tile("waiting", "https://art.example/waiting.jpg", logo = "https://art.example/logo.png")
        compose.waitForIdle()
        val waiting = bounds(SETTINGS_HEADER).height
        holding = false
        art = tile("landed", "https://art.example/landed.jpg", logo = "https://art.example/logo2.png")
        compose.waitForIdle()
        val landed = bounds(SETTINGS_HEADER).height
        assertEquals(expected, none, 0.5f, "no art")
        assertEquals(none, waiting, 0.5f, "art on its way")
        assertEquals(none, landed, 0.5f, "art landed")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the header holds one height with art, before it lands and with none`() =
        oneHeight(PHONE_HEADER.value * 3f) { art -> phone(art) }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the header holds one height with art, before it lands and with none`() =
        oneHeight(TV_HEADER.value) { art -> tv(art) }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the menu and the lists are one column centered on the screen`() {
        serve()
        tv({ null })
        compose.waitForIdle()
        // The 1920 canvas: a 1600 column, 160 in from each side.
        assertEquals(160f, bounds(SETTINGS_MENU).left, 1f)
        assertEquals(160f + TV_MENU.value, bounds(SETTINGS_MENU).right, 1f)
        assertEquals(1760f, bounds(SETTINGS_LIST).right, 1f)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the profile has the first focus, the header leaves past Profiles, the remote crosses`() {
        serve()
        tv({ null })
        compose.waitUntil(5_000) { runCatching { focused().assert(hasTestTag(SETTINGS_PROFILE)) }.isSuccess }
        assertEquals(0f, bounds(SETTINGS_HEADER).top, 1f)

        // Down from the profile: the menu's Profiles entry, the header still in place.
        press(Key.DirectionDown)
        focused().assert(hasText("Profiles") and inMenu)
        assertEquals(0f, bounds(SETTINGS_HEADER).top, 1f)

        // Down again: Sources, the header gone above and the menu at the top.
        press(Key.DirectionDown)
        focused().assert(hasText("Sources") and inMenu)
        assertTrue(bounds(SETTINGS_HEADER).bottom <= 1f, "the header stayed: ${bounds(SETTINGS_HEADER)}")
        assertTrue(bounds(SETTINGS_MENU).top < 100f, "the menu did not rise: ${bounds(SETTINGS_MENU)}")

        // Right into the lists, Left back to the menu's filled entry.
        press(Key.DirectionRight)
        focused().assert(inList)
        press(Key.DirectionLeft)
        focused().assert(hasText("Sources") and inMenu)
        compose.onNode(hasText("Sources") and inMenu).assertIsSelected()

        // Up past Profiles: the profile again, the header back.
        press(Key.DirectionUp)
        press(Key.DirectionUp)
        focused().assert(hasTestTag(SETTINGS_PROFILE))
        assertEquals(0f, bounds(SETTINGS_HEADER).top, 1f)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, a menu entry fills and takes the remote into its section`() {
        serve()
        tv({ null })
        compose.waitUntil(5_000) { runCatching { focused().assert(hasTestTag(SETTINGS_PROFILE)) }.isSuccess }
        compose.onNode(hasText("Playback") and inMenu).performClick()
        compose.waitForIdle()
        focused().assert(inList and hasText("Stream choice"))
        val title = boundsOf(compose.onNode(hasText("Playback") and inList))
        val list = bounds(SETTINGS_LIST)
        assertTrue(abs(title.top - list.top) < 80f, "Playback is not at the top: $title in $list")
        compose.onNode(hasText("Playback") and inMenu).assertIsSelected()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Subtitles and audio is its own menu entry after Playback and takes the remote into its section`() {
        serve()
        tv({ null })
        compose.waitUntil(5_000) { runCatching { focused().assert(hasTestTag(SETTINGS_PROFILE)) }.isSuccess }
        val playback = boundsOf(compose.onNode(hasText("Playback") and inMenu))
        val subtitles = boundsOf(compose.onNode(hasText("Subtitles and audio") and inMenu))
        assertTrue(subtitles.top > playback.top, "Subtitles and audio is not after Playback: $subtitles, $playback")
        compose.onNode(hasText("Subtitles and audio") and inMenu).performClick()
        compose.waitForIdle()
        focused().assert(inList and hasText("Second line"))
        compose.onNode(hasText("Stream choice") and inList).assertDoesNotExist()
        compose.onNode(hasText("Subtitles and audio") and inMenu).assertIsSelected()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the menu shows one section at a time, Sources first`() {
        serve()
        tv({ null })
        compose.waitUntil(5_000) { runCatching { focused().assert(hasTestTag(SETTINGS_PROFILE)) }.isSuccess }
        compose.onNode(hasText("Debrid services") and inList).assertExists()
        compose.onNode(hasText("Stream choice") and inList).assertDoesNotExist()
        compose.onNode(hasText("Playback") and inMenu).performClick()
        compose.waitForIdle()
        compose.onNode(hasText("Stream choice") and inList).assertExists()
        compose.onNode(hasText("Debrid services") and inList).assertDoesNotExist()
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, the list names each section with its line and opens its page`() {
        serve()
        var opened: SettingsSection? = null
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneSettings(me, profileLine(false), null, {}, parts(), onSection = {
                    opened =
                        it
                })
            }
        }
        compose.waitForIdle()
        compose.onNodeWithText("Debrid services").assertDoesNotExist()
        compose.onNodeWithTag(SETTINGS_LIST).performScrollToIndex(1)
        compose.onNodeWithText(SettingsSection.Sources.summary).assertExists()
        compose.onNodeWithText(SettingsSection.Subtitles.summary).assertExists()
        compose.onNodeWithText(SettingsSection.Playback.summary).assertExists()
        compose.onNodeWithText("Sources").performClick()
        compose.runOnIdle { assertEquals(SettingsSection.Sources, opened) }
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, a section's page has its title and its rows`() {
        serve()
        compose.setContent {
            MarqueeTheme(
                Form.Phone,
            ) { PhoneSettings(me, profileLine(false), null, {}, parts(), SettingsSection.Playback) }
        }
        compose.waitForIdle()
        compose.onNodeWithText("Playback").assertExists()
        compose.onNodeWithText("Stream choice").assertExists()
        compose.onNodeWithText("Debrid services").assertDoesNotExist()
        compose.onNodeWithTag(SETTINGS_HEADER).assertDoesNotExist()
    }

    @Test
    fun `a route's section name reads back as its section`() {
        for (s in SettingsSection.entries) assertSame(s, settingsSection(s.name))
        assertNull(settingsSection("Nope"))
        assertNull(settingsSection(null))
    }

    /** WCAG contrast of [ink] on [fill] laid on [art]. */
    private fun contrast(
        ink: Color,
        fill: Color,
        art: Color,
    ): Float {
        val back = fill.compositeOver(art)
        val la = ink.compositeOver(back).luminance() + 0.05f
        val lb = back.luminance() + 0.05f
        return maxOf(la, lb) / minOf(la, lb)
    }

    /**
     * Every word on the header over white art: each at its top trailing corner (its least covered point), the art
     * there covered by the ramps as the page color, then the word's own fill. Native graphics: only they lay text out as a
     * device does.
     */
    private fun overWhite(
        page: Color,
        side: List<ScrimStop>,
        foot: List<ScrimStop>,
        words: List<Triple<SemanticsMatcher, Color, Color>>,
    ) {
        val art = bounds(SETTINGS_ART)
        for ((what, ink, fill) in words) {
            // The words themselves, not the row or button they merge into.
            val box = boundsOf(compose.onNode(what, useUnmergedTree = true))
            val x = (box.right - art.left) / art.width
            val y = (art.bottom - box.top) / art.height
            val covered = 1 - (1 - Scrims.alpha(side, x)) * (1 - Scrims.alpha(foot, y))
            val ratio = contrast(ink, fill, page.copy(alpha = covered).compositeOver(Color.White))
            assertTrue(ratio >= 4.5f, "${what.description}: $ratio")
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, every word on the header keeps 4_5 to 1 over a white backdrop`() {
        serve()
        holding = true
        tv({ tile("white", "https://art.example/white.jpg") })
        compose.waitForIdle()
        val p = formPalette(Form.Tv)
        val clear = Color.Transparent
        overWhite(
            p.bg,
            Scrims.settingsSide,
            Scrims.settingsFoot,
            listOf(
                Triple(hasText("Mia"), Color.White, clear),
                Triple(hasText(profileLine(false)), p.muted, clear),
                Triple(hasText("Edit profile"), p.text, SECONDARY_FILL),
                Triple(hasText("Manage profiles"), p.text, SECONDARY_FILL),
            ),
        )
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, every word on the header keeps 4_5 to 1 over a white backdrop`() {
        serve()
        holding = true
        phone({ tile("white", "https://art.example/white.jpg") })
        compose.waitForIdle()
        val p = formPalette(Form.Phone)
        overWhite(
            p.bg,
            emptyList(),
            Scrims.settingsPhoneFoot,
            listOf(
                Triple(hasText("Settings"), Color.White, Color.Transparent),
                Triple(hasText("Mia"), p.text, PHONE_CARD),
                Triple(hasText(profileLine(false)), p.dim, PHONE_CARD),
            ),
        )
    }

    @Test
    fun `every icon tile's white glyph keeps 3 to 1 on its fill, green the lowest`() {
        val p = Tokens.dark
        val tiles = listOf(p.tileBlue, p.tileSky, p.tileSea, p.tileTeal, p.tileMint, p.tileGreen, p.tileGrey)
        for (fill in tiles) assertTrue(contrast(Color.White, Color.Transparent, fill) >= 3f, "$fill")
        assertEquals(3.13f, contrast(Color.White, Color.Transparent, p.tileGreen), 0.01f)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone render`() {
        serve(scene())
        phone({ tile("Big Buck Bunny", "https://art.example/scene.jpg") })
        compose.waitForIdle()
        artLanded()
        shot("settings-phone")
        compose.onNodeWithTag(SETTINGS_LIST).performScrollToIndex(1)
        compose.waitForIdle()
        shot("settings-phone-scrolled")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone render with no art`() {
        serve()
        phone({ null })
        compose.waitForIdle()
        shot("settings-phone-no-art")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV render`() {
        serve(scene())
        tv({ tile("Big Buck Bunny", "https://art.example/scene.jpg") })
        compose.waitForIdle()
        artLanded()
        shot("settings-tv")
    }
}
