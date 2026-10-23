package app.marquee.android

import app.marquee.core.client.CoreLog
import org.junit.After
import org.junit.Test
import java.io.File
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * plans/play-every-version.md, Phase 0: the build says own or store; an own build's core carries the engine. A
 * mismatch never puts developer words before the viewer: Settings, About, a log line and a debug assertion tell
 * the owner, as on Apple.
 */
class CoreBuildTest {
    private val logs = mutableListOf<String>()

    @After
    fun quiet() {
        CoreLog.sink = { _, _, _ -> }
        CoreBuild.check("own", ENGINE_ABIS, ARM64, hasEngine = true, debug = false)
    }

    @Test
    fun `an own build without the engine is a mismatch, a store build is not`() {
        assertTrue(CoreBuild.missingEngine("own", ENGINE_ABIS, ARM64, hasEngine = false))
        assertFalse(CoreBuild.missingEngine("own", ENGINE_ABIS, ARM64, hasEngine = true))
        assertFalse(CoreBuild.missingEngine("store", "", ARM64, hasEngine = false), "a store core, on purpose")
    }

    @Test
    fun `an armeabi-v7a device has no engine by design, so nothing is missing`() {
        CoreLog.sink = { level, message, _ -> logs += "$level $message" }
        assertFalse(CoreBuild.missingEngine("own", ENGINE_ABIS, "armeabi-v7a", hasEngine = false))
        // A 32-bit stick: no log line, no About line, and a debug build goes on.
        CoreBuild.check("own", ENGINE_ABIS, "armeabi-v7a", hasEngine = false, debug = true)
        assertFalse(CoreBuild.missing)
        assertEquals(emptyList(), logs)
    }

    @Test
    fun `a mismatch is logged and shown in About, and a debug build stops on it`() {
        CoreLog.sink = { level, message, _ -> logs += "$level $message" }
        CoreBuild.check("own", ENGINE_ABIS, ARM64, hasEngine = false, debug = false)
        assertTrue(CoreBuild.missing)
        assertEquals(listOf("e core build own: This build's core has no torrent engine"), logs)
        val stopped =
            assertFailsWith<IllegalStateException> {
                CoreBuild.check("own", ENGINE_ABIS, ARM64, hasEngine = false, debug = true)
            }
        assertEquals(CoreBuild.ABOUT_LINE, stopped.message)
        CoreBuild.check("store", "", ARM64, hasEngine = false, debug = true)
        assertFalse(CoreBuild.missing)
    }

    @Test
    fun `the mismatch reaches the owner, not the viewer`() {
        val app = "src/main/kotlin/app/marquee/android"
        assertTrue(
            "BuildConfig.CORE_ENGINE_ABIS,\n                    Build.SUPPORTED_ABIS[0]," in
                File("$app/MarqueeApp.kt").readText(),
        )
        assertTrue(
            "if (CoreBuild.missing) MqText(CoreBuild.ABOUT_LINE" in File("$app/SettingsScreen.kt").readText(),
        )
        // The player and the title page say nothing of their own about it: the core's account state is the viewer's.
        for (path in listOf("$app/PlayerScreen.kt", "$app/TitleScreens.kt")) {
            assertFalse("CoreBuild" in File(path).readText())
        }
    }

    private companion object {
        const val ARM64 = "arm64-v8a"

        /** As scripts/build-core.sh writes it for an own build of both ABIs. */
        const val ENGINE_ABIS = "arm64-v8a"
    }
}
