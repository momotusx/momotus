package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import android.os.Looper
import android.view.View
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.unit.dp
import androidx.navigation3.runtime.NavKey
import app.marquee.core.AccountCopy
import app.marquee.core.FirstProfileStep
import app.marquee.core.FirstRun
import app.marquee.core.FirstRunCopy
import app.marquee.core.FirstSetup
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.PosterWall
import app.marquee.design.WallDrift
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.Uri
import coil3.asImage
import coil3.decode.DataSource
import coil3.fetch.FetchResult
import coil3.fetch.Fetcher
import coil3.fetch.ImageFetchResult
import coil3.request.Options
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
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
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * First run's screens on Android (plans/first-screens.md 3, 5, 9; vectors parts b and c): every path's screens and
 * presses through FirstRunNav and the shipped Welcome, Login sheet (LoginSheet and loginStart, SignInScreen's own),
 * profile form, setup sheet, TV Login sheet and the picker (PickerScreen over a ProfileGate). Stand-ins: the relay's
 * answers (AccountSignIn's steps are AccountSignInTest's), the TV pairing code and the other device's join. Also
 * the untouched Continue, the first focus on TV, 48 dp targets, no addon name, the stacks Back walks, a large font
 * and the chip rows. With MQ_SHOTS set, the render tests save theirs there.
 */
@OptIn(ExperimentalLayoutApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class FirstScreensTest {
    @get:Rule
    val compose = createComposeRule()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val gate = ProfileGate(session)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    private val prefs =
        CoreJson
            .parseToJsonElement(File("../core/src/test/resources/traces/settings.jsonl").readLines()[1])
            .jsonObject
            .getValue("msg")
            .jsonObject
            .getValue("result")
            .jsonObject
            .getValue("snapshot")
            .toString()

    /** The defaults the fake core recommends: no first-run screen may name any of them. */
    private val addonNames = listOf("Cinemeta", "Torrentio", "OpenSubtitles")
    private val defaults =
        """{"version":1,"addons":[""" +
            """{"key":"cinemeta","name":"Cinemeta","line":"Titles.","group":"catalogs","installed":"absent"},""" +
            """{"key":"torrentio","name":"Torrentio","line":"Streams.","group":"streams","installed":"absent"},""" +
            """{"key":"opensubtitles","name":"OpenSubtitles","line":"Subs.",""" +
            """"group":"subtitles","installed":"absent"}]}"""

    private val owner =
        ProfileInfo(
            id = "default",
            name = "Main",
            avatar = "",
            position = 0,
            hasPin = false,
            isOwner = true,
            syncing = false,
            kids = false,
        )

    private val languages =
        listOf(LanguageChoice("en", "English"), LanguageChoice("tr", "Turkish"), LanguageChoice("de", "German"))

    @Before
    fun attach() {
        session.client.attach(
            CoreTransport { text ->
                val msg = CoreJson.parseToJsonElement(text).jsonObject
                sent += msg
                val id = msg["id"]?.jsonPrimitive?.long ?: return@CoreTransport
                val result =
                    when (msg.getValue("op").jsonPrimitive.content) {
                        "finishProfileStep", "createProfile" -> {
                            """{"profiles":[],"active":"default","locked":false,"max":5,"kids":false}"""
                        }

                        "setPrefs" -> {
                            prefs
                        }

                        "defaultAddons" -> {
                            defaults
                        }

                        "firstRunWall" -> {
                            "[]"
                        }

                        // The profiles view after a join to two profiles: Who's watching? opens on the start picker.
                        "subscribe" -> {
                            val who =
                                ProfilesSnapshot(
                                    listOf(owner, owner.copy(id = "p2", name = "Deniz", position = 1, isOwner = false)),
                                    active = owner.id,
                                    locked = false,
                                    max = 5,
                                    kids = false,
                                )
                            """{"sub":9,"snapshot":${CoreJson.encodeToString(ProfilesSnapshot.serializer(), who)}}"""
                        }

                        else -> {
                            "{}"
                        }
                    }
                session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
            },
        )
        session.opened(running = true)
        gate.start()
    }

    @After
    fun reset() {
        SingletonImageLoader.reset()
        RuntimeEnvironment.setFontScale(1f)
    }

    private fun ops(name: String) = sent.filter { it.getValue("op").jsonPrimitive.content == name }

    /** One first run on a stack, as AppNav drives it; the other device's join is [join]. */
    private inner class Run(
        val form: Form,
        val server: Boolean,
    ) {
        val stack = mutableStateListOf<NavKey>()
        val screens = mutableListOf<String>()
        var inAccount = false
        var stepDone = false
        var profiles = 1
        var home by mutableStateOf(false)
        var who by mutableStateOf(false)
        val nav =
            FirstRunNav(
                stack,
                form == Form.Tv,
                state = {
                    FirstRun.State(true, false, server, inAccount, stepDone, profiles, pin = false, locked = false)
                },
                onHome = { home = true },
                onWho = { who = true },
            )

        init {
            stack.addAll(FirstRunNav.initial(FirstRun.State(true, false, server, false, false, 1, false, false)))
        }

        /** The other device approved: AppNav's pull, then the rule. */
        fun join(count: Int) {
            inAccount = true
            profiles = count
            nav.go(FirstRun.After.Joined)
        }

        val screen: String
            get() =
                if (who) {
                    "who"
                } else if (home) {
                    "home"
                } else {
                    top(stack.lastOrNull())
                }

        private fun top(top: NavKey?) =
            when (top) {
                Route.Welcome -> "welcome"
                is Route.SignIn, Route.TvSignIn -> "login"
                Route.FirstProfile -> "profile"
                Route.Setup -> "setup"
                Route.Import -> "import"
                else -> "unknown $top"
            }
    }

    private var run by mutableStateOf<Run?>(null)

    @Composable
    private fun Host(r: Run) {
        val scope = rememberCoroutineScope()
        val step = remember(r) { FirstProfileStep(session.ops, scope) }
        val setup = remember(r) { FirstSetup(session.ops, scope) }
        // Held still: a drifting wall never lets the test idle (its frames never end).
        val drift = remember { WallDrift().apply { paused = true } }
        val screen = r.screen
        LaunchedEffect(r, screen) { if (screen != "home") r.screens += screen }
        Box(Modifier.fillMaxSize()) {
            PosterWall(emptyList(), drift, sheet = screen != "welcome")
            when (screen) {
                "welcome" -> {
                    WelcomeScreen(
                        drift,
                        { r.nav.go(FirstRun.After.GetStarted) },
                        if (r.server) ({ r.nav.go(FirstRun.After.AlreadyUse) }) else null,
                    )
                }

                "login" -> {
                    if (r.form == Form.Tv) {
                        TvLoginSheet(null, false, null, { r.nav.go(FirstRun.After.NotNow) }, {}, null)
                    } else {
                        PhoneLogin(r, (r.stack.last() as Route.SignIn).returning)
                    }
                }

                "profile" -> {
                    FirstProfileSheet(owner, r.inAccount, step.busy, step.problem, null) { name, avatar ->
                        step.finish(name, avatar) {
                            r.stepDone = true
                            r.nav.go(FirstRun.After.ProfileDone)
                        }
                    }
                }

                "setup" -> {
                    SetupSheet(
                        setup,
                        "Main",
                        SetupChips(FirstRun.chips("en", emptyList()), languages),
                        onStart = { subtitle, chosen -> setup.start(subtitle, chosen, r.nav::setupDone) },
                        onSkip = { setup.skip { r.nav.setupDone(false) } },
                        onReview = {},
                    )
                }

                "who" -> {
                    // The shipped picker, as the gate draws it over the wall after a join.
                    LaunchedEffect(gate.open) { if (gate.open) r.home = true }
                    PickerScreen(
                        gate,
                        session,
                        PickerMode.Start,
                        onAdd = {},
                        sheet = true,
                        line = FirstRunCopy.WHO_SIGNED_IN,
                    )
                }

                else -> {
                    Unit
                }
            }
        }
    }

    /**
     * The phone Login sheet as SignInScreen draws its first stage: the shipped LoginSheet and loginStart. Stand-ins:
     * the relay's provider list (Google, Facebook and email, as AccountSignIn would offer them) and its answers (a
     * Google press is an account made at once, 2026-11-05); the pairing code under Use another device is not drawn.
     */
    @Composable
    private fun PhoneLogin(
        r: Run,
        returning: Boolean,
    ) {
        var another by remember { mutableStateOf(false) }
        LoginSheet(
            AccountCopy.SIGN_IN_TITLE,
            start = true,
            returning = returning,
            onNotNow = { r.nav.go(FirstRun.After.NotNow) },
            onBack = {},
        ) {
            loginStart(
                listOf("google", "facebook"),
                email = true,
                continueHere = null,
                returning = returning,
                another = another,
                onAnother = { another = !another },
                pairing = {},
                choose = { provider ->
                    if (provider == "google") {
                        r.inAccount = true
                        r.nav.go(FirstRun.After.Created)
                    }
                },
                onEmail = {},
                others = false,
                onOthers = {},
            ) {}
        }
    }

    /** The control a press of the vectors names. */
    private fun control(press: String) =
        when (press) {
            "getStarted" -> compose.onNodeWithText(FirstRunCopy.GET_STARTED)
            "alreadyUse" -> compose.onNodeWithText(FirstRunCopy.ALREADY_USE)
            "notNow" -> compose.onNodeWithText(AccountCopy.NOT_NOW)
            "continueWithGoogle" -> compose.onNodeWithText("Continue with Google")
            "continue" -> compose.onNodeWithText(FirstRunCopy.CONTINUE)
            "bringLibrary" -> compose.onNodeWithText(FirstRunCopy.BRING_LIBRARY)
            "startWatching" -> compose.onNodeWithText(FirstRunCopy.START_WATCHING)
            "useAnotherDevice" -> compose.onNodeWithText(FirstRunCopy.USE_ANOTHER_DEVICE)
            "profileTile" -> compose.onAllNodes(hasContentDescription("Main", substring = true)).onFirst()
            else -> error("unknown press $press")
        }

    private val vectors =
        CoreJson.parseToJsonElement(File("../../shared/fixtures/first-run-vectors.json").readText()).jsonObject

    private fun idle() {
        repeat(20) {
            shadowOf(Looper.getMainLooper()).idle()
            compose.waitForIdle()
            Thread.sleep(5)
        }
    }

    private fun walkPaths(form: Form) {
        compose.setContent { MarqueeTheme(form) { run?.let { Host(it) } } }
        val key = if (form == Form.Tv) "tv" else "phone"
        for (path in vectors.getValue("paths").jsonArray.map { it.jsonObject }) {
            val name = path.getValue("path").jsonPrimitive.content
            val row = path[key]?.jsonObject ?: continue
            val expected = row.getValue("screens").jsonArray.map { it.jsonPrimitive.content }
            val presses = row.getValue("presses").jsonArray.map { it.jsonPrimitive.content }
            val moves =
                row["moves"]
                    ?.takeUnless { it is JsonNull }
                    ?.jsonPrimitive
                    ?.content
                    ?.toInt()
            val r = Run(form, server = name != "noServer")
            run = r
            idle()
            var pressed = 0
            for (press in presses) {
                // The other device approves once this device has shown its code; Who's watching? opens after it.
                if (press == "profileTile") r.join(3)
                idle()
                val target = control(press)
                runCatching { target.performScrollTo() }
                if (form == Form.Tv && moves == 0) {
                    compose.waitUntil(5_000) { runCatching { target.assert(isFocused()) }.isSuccess }
                }
                if (form == Form.Tv && name.startsWith("join") && press == "alreadyUse") {
                    // One move right from Get started (the vectors' D-pad column).
                    compose.waitUntil(5_000) { runCatching { control("getStarted").assert(isFocused()) }.isSuccess }
                    control("getStarted").performKeyInput { pressKey(Key.DirectionRight) }
                    compose.waitUntil(5_000) { runCatching { target.assert(isFocused()) }.isSuccess }
                }
                target.performClick()
                pressed++
                idle()
            }
            if (name == "joinOneProfile") r.join(1)
            idle()
            compose.waitUntil(5_000) { r.home || r.screen == "import" }
            assertEquals(expected, r.screens, "$name on $key")
            assertEquals(presses.size, pressed, "$name on $key")
        }
        assertTrue(ops("createProfile").isEmpty(), "first run never makes a profile")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, every path shows its screens for its presses and never makes a profile`() = walkPaths(Form.Phone)

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, every path shows its screens for its presses, each on the first focus`() = walkPaths(Form.Tv)

    @Test
    fun `Back walks the stacks of plans first-screens 5`() {
        val r = Run(Form.Phone, server = true)
        assertEquals(listOf<NavKey>(Route.Welcome), r.stack.toList())
        r.nav.go(FirstRun.After.GetStarted)
        r.nav.go(FirstRun.After.NotNow)
        // Not now: Back from the profile form is the Login sheet.
        assertEquals(listOf(Route.Welcome, Route.SignIn(firstRun = true), Route.FirstProfile), r.stack.toList())
        r.nav.go(FirstRun.After.ProfileDone)
        assertEquals(Route.FirstProfile, r.stack[r.stack.lastIndex - 1])
        r.nav.setupDone(import = true)
        assertEquals(listOf(Route.FirstProfile, Route.Setup, Route.Import), r.stack.takeLast(3))
        // An account made here: the form is the root, so Back leaves the app.
        val made = Run(Form.Phone, server = true)
        made.nav.go(FirstRun.After.GetStarted)
        made.inAccount = true
        made.nav.go(FirstRun.After.Created)
        assertEquals(listOf<NavKey>(Route.FirstProfile), made.stack.toList())
        // No sync server: Get started goes to the form, Back to Welcome.
        val alone = Run(Form.Phone, server = false)
        alone.nav.go(FirstRun.After.GetStarted)
        assertEquals(listOf(Route.Welcome, Route.FirstProfile), alone.stack.toList())
        // I already use Momotus: the returning sheet; TV its own.
        r.stack.clear()
        r.stack.add(Route.Welcome)
        r.nav.go(FirstRun.After.AlreadyUse)
        assertEquals(Route.SignIn(firstRun = true, returning = true), r.stack.last())
        val tv = Run(Form.Tv, server = true)
        tv.nav.go(FirstRun.After.AlreadyUse)
        assertEquals(Route.TvSignIn, tv.stack.last())
        // A restart with the profile step answered: Back from setup is the form.
        val restart = FirstRun.State(true, false, true, false, profileStepDone = true, 1, false, false)
        assertEquals(listOf<Route>(Route.FirstProfile, Route.Setup), FirstRunNav.initial(restart))
        // A sign-in pushed over the form (a pairing link): its Not now comes back to that form, not a second one.
        val linked = Run(Form.Phone, server = true)
        linked.stack.add(Route.FirstProfile)
        linked.stack.add(Route.SignIn(firstRun = true))
        linked.nav.go(FirstRun.After.NotNow)
        assertEquals(listOf(Route.Welcome, Route.FirstProfile), linked.stack.toList())
    }

    @Test
    fun `a join goes on by the rule only with no sign-in open and no account made here`() {
        assertTrue(joinGoesOn(joined = true, firstRun = true, signingIn = false, created = false))
        // Back from a stage after the account was made: the sign-in is still open, so the made account is not a join.
        assertTrue(!joinGoesOn(joined = true, firstRun = true, signingIn = true, created = false))
        assertTrue(!joinGoesOn(joined = true, firstRun = true, signingIn = false, created = true))
        assertTrue(!joinGoesOn(joined = true, firstRun = false, signingIn = false, created = false))
        assertTrue(!joinGoesOn(joined = false, firstRun = true, signingIn = false, created = false))
    }

    @Test
    fun `the wall shows on first run's sheets and every sign-in, never on a setup opened from Home`() {
        val home = listOf(Route.Home, Route.Setup)
        assertTrue(!wallShows(Route.Setup, home, firstRun = false))
        assertTrue(sheetTop(Route.Setup, home))
        assertTrue(wallShows(Route.Setup, listOf(Route.FirstProfile, Route.Setup), firstRun = true))
        assertTrue(
            wallShows(Route.SignIn(firstRun = false), listOf(Route.Settings, Route.SignIn(false)), firstRun = false),
        )
        assertTrue(wallShows(Route.TvSignIn, listOf(Route.Settings, Route.TvSignIn), firstRun = false))
        assertTrue(!wallShows(Route.Import, listOf(Route.Settings, Route.Import), firstRun = false))
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `Getting your profiles holds taps and the remote's keys from the sheet under it`() {
        var pressed = 0
        var pulling by mutableStateOf(false)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                Box(Modifier.fillMaxSize()) {
                    TvLoginSheet(null, false, null, { pressed++ }, { pressed++ }, null)
                    if (pulling) GettingProfiles()
                }
            }
        }
        // As in the app: Not now has the focus when the join comes and the pull begins.
        compose.waitUntil(
            5_000,
        ) { runCatching { compose.onNodeWithText(AccountCopy.NOT_NOW).assert(isFocused()) }.isSuccess }
        pulling = true
        idle()
        // The layer has the focus, not Not now under it, and OK goes nowhere.
        compose.waitUntil(5_000) {
            runCatching { compose.onNodeWithTag(GETTING_PROFILES_LAYER).assert(isFocused()) }.isSuccess
        }
        compose.onNodeWithTag(GETTING_PROFILES_LAYER).performKeyInput {
            pressKey(Key.DirectionCenter)
            pressKey(Key.DirectionDown)
            pressKey(Key.Enter)
        }
        compose.onNodeWithText(AccountCopy.NOT_NOW).performClick()
        idle()
        assertEquals(0, pressed)
        compose
            .onNodeWithText(
                AccountCopy.NOT_NOW,
            ).assert(SemanticsMatcher.expectValue(SemanticsProperties.Focused, false))
    }

    @Test
    fun `the benchmarks walk the new first run to Home`() {
        val flows = File("../macrobenchmark/src/main/kotlin/app/marquee/macrobenchmark/Flows.kt").readText()
        val walk = flows.substringAfter("fun MacrobenchmarkScope.onboardIfAsked()").substringBefore("\n}\n")
        val presses =
            listOf(FirstRunCopy.GET_STARTED, AccountCopy.NOT_NOW, FirstRunCopy.CONTINUE, FirstRunCopy.START_WATCHING)
        val at = presses.map { walk.indexOf("By.text(\"$it\")") }
        assertTrue(at.all { it >= 0 } && at == at.sorted(), "Flows.kt presses $presses at $at")
        assertTrue("Popular" in walk)
    }

    @Test
    fun `the baseline profiles name only routes that exist, in rule form`() {
        val rule = Regex("""^[HSP]*L[\w/$-]+;(->.+)?$""")
        for (name in listOf("baseline-prof.txt", "startup-prof.txt")) {
            val lines = File("src/main/generated/baselineProfiles/$name").readLines().filter { it.isNotBlank() }
            for (line in lines) {
                assertTrue(rule.matches(line) && !Regex("""^[HSP]*LL""").containsMatchIn(line), "$name: $line")
            }
            val routes =
                lines
                    .mapNotNull { Regex("""core/route/Route\$(\w+);""").find(it)?.groupValues?.get(1) }
                    .toSet()
            for (route in routes) Class.forName("app.marquee.core.route.Route\$$route")
            assertTrue(setOf("Welcome", "FirstProfile", "Setup").all { it in routes }, "$name lists $routes")
        }
    }

    private fun profileForm(form: Form) =
        compose.setContent {
            MarqueeTheme(form) {
                val scope = rememberCoroutineScope()
                val step = remember { FirstProfileStep(session.ops, scope) }
                FirstProfileSheet(owner, false, step.busy, step.problem, null) { n, a -> step.finish(n, a) {} }
            }
        }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `an untouched Continue sends an empty name and avatar, and the field is not focused on open`() {
        profileForm(Form.Phone)
        idle()
        compose.onNode(isFocused()).assertDoesNotExist()
        compose.onNodeWithText(FirstRunCopy.CONTINUE).performClick()
        compose.waitUntil(5_000) { ops("finishProfileStep").isNotEmpty() }
        val asked = ops("finishProfileStep").single()
        assertEquals("", asked.getValue("name").jsonPrimitive.content)
        assertEquals("", asked.getValue("avatar").jsonPrimitive.content)
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `a typed name and a pressed colour are what Continue sends`() {
        profileForm(Form.Phone)
        idle()
        val field = hasText(FirstRunCopy.NAME_PLACEHOLDER) or hasContentDescription(FirstRunCopy.NAME_LABEL)
        compose.onNode(field and SemanticsMatcher.keyIsDefined(SemanticsActions.SetText)).performTextInput("Deniz")
        compose.onNode(hasContentDescription(SWATCHES[2].replaceFirstChar { it.uppercase() })).performClick()
        compose.onNodeWithText(FirstRunCopy.CONTINUE).performClick()
        compose.waitUntil(5_000) { ops("finishProfileStep").isNotEmpty() }
        val asked = ops("finishProfileStep").single()
        assertEquals("Deniz", asked.getValue("name").jsonPrimitive.content)
        assertEquals(ProfileCopy.formatAvatar("letter", SWATCHES[2]), asked.getValue("avatar").jsonPrimitive.content)
    }

    /**
     * Pixel walk 2026-11-09 L32: the TV form on a touch device starts in touch mode, where a button takes no focus,
     * so Welcome's first focus never landed and the first OK went to Pause motion at the top. Get started has it.
     */
    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV Welcome's first focus is Get started, from touch mode too`() {
        var window: View? = null
        var shown by mutableStateOf(false)
        compose.setContent {
            window = LocalView.current
            MarqueeTheme(Form.Tv) { if (shown) WelcomeScreen(WallDrift(), {}, {}) }
        }
        // As a phone starts: the window in touch mode, through the platform's own call.
        compose.runOnUiThread {
            val root = View::class.java.getMethod("getViewRootImpl").invoke(window)
            root.javaClass
                .getDeclaredMethod("ensureTouchMode", Boolean::class.javaPrimitiveType)
                .apply { isAccessible = true }
                .invoke(root, true)
        }
        assertTrue(window!!.isInTouchMode, "the window is in touch mode")
        shown = true
        compose.waitUntil(5_000) {
            runCatching { compose.onNodeWithText(FirstRunCopy.GET_STARTED).assert(isFocused()) }.isSuccess
        }
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Continue, Start watching and Not now take the first focus`() {
        var screen by mutableStateOf("profile")
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                val scope = rememberCoroutineScope()
                when (screen) {
                    "profile" -> {
                        FirstProfileSheet(owner, false, false, null, null) { _, _ -> }
                    }

                    "setup" -> {
                        SetupSheet(
                            remember { FirstSetup(session.ops, scope) },
                            "Main",
                            SetupChips(FirstRun.chips("en", emptyList()), languages),
                            { _, _ -> },
                            {},
                            {},
                        )
                    }

                    else -> {
                        TvLoginSheet(null, false, AccountCopy.UNREACHABLE, {}, {}, {})
                    }
                }
            }
        }

        fun focused(text: String) =
            compose.waitUntil(5_000) { runCatching { compose.onNodeWithText(text).assert(isFocused()) }.isSuccess }
        focused(FirstRunCopy.CONTINUE)
        screen = "setup"
        focused(FirstRunCopy.START_WATCHING)
        screen = "login"
        focused(AccountCopy.NOT_NOW)
        compose.onNodeWithText(FirstRunCopy.TV_NO_QR, useUnmergedTree = true).assertExists()
        compose.onNodeWithText(FirstRunCopy.TRY_AGAIN).assertExists()
    }

    /** Every control on screen is at least 48 dp each way. */
    private fun targets() {
        val min = with(compose.density) { 48.dp.toPx() } - 0.5f
        val clickable =
            compose.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.OnClick)).fetchSemanticsNodes()
        assertTrue(clickable.isNotEmpty())
        // A text field is measured on its 48 dp box (MqTextField), not on the line of text inside it.
        for (node in clickable.filterNot { SemanticsProperties.EditableText in it.config }) {
            // Its own size: a control half scrolled out of a short sheet is clipped on screen, not smaller.
            val b = node.size
            assertTrue(b.width >= min && b.height >= min, "${node.config} is $b")
        }
    }

    /** No default addon is named on a first-run screen (D12: only Import and Review do). */
    private fun noAddonNames() {
        for (name in addonNames) compose.onAllNodesWithText(name, substring = true).assertCountEquals(0)
    }

    private fun eachScreen(
        form: Form,
        check: () -> Unit,
    ) {
        val r = Run(form, server = true)
        run = r
        compose.setContent { MarqueeTheme(form) { run?.let { Host(it) } } }
        idle()
        check()
        r.nav.go(FirstRun.After.GetStarted)
        idle()
        check()
        r.nav.go(FirstRun.After.NotNow)
        idle()
        check()
        r.nav.go(FirstRun.After.ProfileDone)
        idle()
        check()
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, every control is 48 dp and no addon is named`() =
        eachScreen(Form.Phone) {
            targets()
            noAddonNames()
        }

    @Test
    @Config(qualifiers = "w844dp-h390dp-xxhdpi")
    fun `phone on its side, every control is 48 dp`() = eachScreen(Form.Phone) { targets() }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, no addon is named`() = eachScreen(Form.Tv) { noAddonNames() }

    @Test
    @Config(qualifiers = "w360dp-h740dp-xhdpi")
    fun `at font scale 2 the setup sheet scrolls and Start watching stays in sight`() {
        RuntimeEnvironment.setFontScale(2f)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                SetupSheet(
                    FirstSetup(session.ops, rememberCoroutineScope()),
                    "A thirty character profile nm",
                    SetupChips(FirstRun.chips("tr", listOf("de")), languages),
                    { _, _ -> },
                    {},
                    {},
                )
            }
        }
        idle()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val start = compose.onNodeWithText(FirstRunCopy.START_WATCHING).fetchSemanticsNode().boundsInRoot
        assertTrue(start.top >= root.top && start.bottom <= root.bottom, "Start watching $start in $root")
        compose.onNodeWithText(FirstRunCopy.MORE_LANGUAGES).performScrollTo()
        val more = compose.onNodeWithText(FirstRunCopy.MORE_LANGUAGES).fetchSemanticsNode().boundsInRoot
        assertTrue(more.bottom <= start.top + 1f, "More $more is under the foot $start")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `every chip row shows its chips in order with the one chosen`() {
        var chips by mutableStateOf(SetupChips(FirstRun.chips(null, emptyList()), languages))
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                SetupSheet(FirstSetup(session.ops, rememberCoroutineScope()), "Main", chips, { _, _ -> }, {}, {})
            }
        }
        for (row in vectors.getValue("chips").jsonArray.map { it.jsonObject }) {
            val device =
                row
                    .getValue("device")
                    .takeUnless { it is JsonNull }
                    ?.jsonPrimitive
                    ?.content
            val chosen = row.getValue("chosen").jsonArray.map { it.jsonPrimitive.content }
            chips = SetupChips(FirstRun.chips(device, chosen), languages)
            idle()
            val codes = row.getValue("chips").jsonArray.map { it.jsonPrimitive.content }
            val places: List<Rect> =
                codes.map {
                    compose
                        .onNodeWithText(chips.name(it))
                        .performScrollTo()
                        .fetchSemanticsNode()
                        .boundsInRoot
                }
            val ordered =
                places.zipWithNext().all { (a, b) -> a.top < b.top - 1f || (a.top <= b.top + 1f && a.left < b.left) }
            assertTrue(ordered, "$row at $places")
            val selected = row.getValue("selected").jsonPrimitive.content
            for (code in codes - FirstRun.MORE) {
                val node = compose.onNodeWithText(chips.name(code)).fetchSemanticsNode()
                val on = node.config.getOrElse(SemanticsProperties.Selected) { false }
                assertEquals(code == selected, on, "$code in $row")
            }
        }
    }

    /** WCAG contrast of [a] over [b]. */
    private fun contrast(
        a: Color,
        b: Color,
    ): Float {
        val (hi, lo) = listOf(a.luminance(), b.luminance()).sortedDescending()
        return (hi + 0.05f) / (lo + 0.05f)
    }

    private fun serveWhite() {
        val image =
            Bitmap
                .createBitmap(40, 60, Bitmap.Config.ARGB_8888)
                .apply { eraseColor(android.graphics.Color.WHITE) }
                .asImage()
        val factory =
            Fetcher.Factory<Uri> { _, _: Options, _: ImageLoader ->
                Fetcher { ImageFetchResult(image, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
    }

    private fun welcomeOverWhite(form: Form) {
        serveWhite()
        compose.setContent {
            MarqueeTheme(form) {
                val drift = remember { WallDrift().apply { paused = true } }
                Box(Modifier.fillMaxSize()) {
                    PosterWall(List(30) { "https://art.example/$it.jpg" }, drift, sheet = false)
                    WelcomeScreen(drift, {}, {})
                }
            }
        }
        compose.mainClock.advanceTimeBy(2_000)
        idle()
        val title = compose.onNodeWithText(FirstRunCopy.WELCOME_TITLE).fetchSemanticsNode().boundsInRoot
        val lead = compose.onNodeWithText(FirstRunCopy.WELCOME_LEAD).fetchSemanticsNode().boundsInRoot
        val pixels = compose.onRoot().captureToImage().toPixelMap()
        // Over white posters the ramp under the words: just before each line, at its middle.
        for (b in listOf(title, lead)) {
            val under = pixels[maxOf(0, b.left.toInt() - 4), b.center.y.toInt()]
            assertTrue(contrast(Color.White, under) >= 4.5f, "${contrast(Color.White, under)} at $b: $under")
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone, Welcome's words keep 4_5 to 1 over a white wall`() = welcomeOverWhite(Form.Phone)

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Welcome's words keep 4_5 to 1 over a white wall`() = welcomeOverWhite(Form.Tv)

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    private fun renders(
        form: Form,
        tag: String,
    ) {
        serveWhite()
        val r = Run(form, server = true)
        run = r
        compose.setContent { MarqueeTheme(form) { run?.let { Host(it) } } }
        idle()
        shot("welcome-$tag")
        r.nav.go(FirstRun.After.GetStarted)
        idle()
        shot("login-$tag")
        r.nav.go(FirstRun.After.NotNow)
        idle()
        shot("profile-$tag")
        r.nav.go(FirstRun.After.ProfileDone)
        idle()
        shot("setup-$tag")
        r.join(3)
        idle()
        shot("who-$tag")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `phone renders`() = renders(Form.Phone, "phone")

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV renders`() = renders(Form.Tv, "tv")
}
