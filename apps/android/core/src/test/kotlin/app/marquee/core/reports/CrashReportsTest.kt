package app.marquee.core.reports

import android.app.ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED
import android.app.ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
import android.app.ApplicationExitInfo
import app.marquee.core.ReportCopy
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Android's side of error reports (plans/error-reports.md 6.2, 10). */
class CrashReportsTest {
    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared =
            Json
                .parseToJsonElement(File("../../shared/report-words.json").readText())
                .jsonObject
                .filterKeys { it != "about" }
                .mapValues { it.value.jsonPrimitive.content }
        assertEquals(shared, ReportCopy.byKey)
        assertEquals("https://relay.example/privacy", ReportCopy.privacyUrl("https://relay.example/"))
        assertNull(ReportCopy.privacyUrl(""), "no sync service, no privacy row")
    }

    @Test
    fun `an ANR keeps the main thread's frames only`() {
        val trace =
            """
            ----- pid 123 -----
            "Signal Catcher" daemon prio=10 tid=4 Runnable
              at java.lang.Thread.sleep(Native Method)

            "main" prio=5 tid=1 Blocked
              | group="main" sCount=1
              at app.marquee.android.HomeScreen.load(HomeScreen.kt:120)
              - waiting to lock <0x0abc> (a java.lang.Object) held by thread 12
              at android.os.Handler.dispatchMessage(Handler.java:106)

            "DefaultDispatcher-worker-1" prio=5 tid=12 Runnable
              at kotlinx.coroutines.X.run(X.kt:1)
            """.trimIndent()
        assertEquals(
            listOf(
                "app.marquee.android.HomeScreen.load(HomeScreen.kt:120)",
                "android.os.Handler.dispatchMessage(Handler.java:106)",
            ),
            mainThreadFrames(trace),
        )
        assertEquals(emptyList(), mainThreadFrames("no main thread here"))
    }

    private fun record(
        reason: Int,
        importance: Int = IMPORTANCE_FOREGROUND,
        status: Int = 0,
    ) = ExitRecord(7, reason, status, importance, "desc", 512L * 1024, 640L * 1024, null)

    @Test
    fun `exit records become reports only for what the core cannot see`() {
        assertEquals(
            "crash",
            exitReport(
                record(ApplicationExitInfo.REASON_CRASH_NATIVE, status = 11),
                true,
            )!!["kind"]!!.jsonPrimitive.content,
        )
        assertEquals("anr", exitReport(record(ApplicationExitInfo.REASON_ANR), true)!!["kind"]!!.jsonPrimitive.content)
        val oom = exitReport(record(ApplicationExitInfo.REASON_LOW_MEMORY), true)!!
        assertEquals("oom", oom["kind"]!!.jsonPrimitive.content)
        assertTrue(oom["message"]!!.jsonPrimitive.content.contains("last sampled pss 512 MB"))
        // A JVM crash is the handler's; a low-memory kill in the background is the system's business.
        assertNull(exitReport(record(ApplicationExitInfo.REASON_CRASH), true))
        assertNull(exitReport(record(ApplicationExitInfo.REASON_LOW_MEMORY, IMPORTANCE_CACHED), true))
        // Where the device cannot report low-memory kills, a foreground SIGKILL stands for one.
        val signaled = record(ApplicationExitInfo.REASON_SIGNALED, status = 9)
        assertEquals("oom", exitReport(signaled, false)!!["kind"]!!.jsonPrimitive.content)
        assertNull(exitReport(signaled, true))
    }

    @Test
    fun `a JVM crash file keeps the root cause's frames, and no text in Incognito`() {
        val root = IllegalStateException("no row for The Matrix")
        val error = RuntimeException("wrapper", root)
        val outside = jvmReport("main", error, 5, incognito = false)
        val report = outside["report"]!!.jsonObject
        assertEquals(
            "java.lang.IllegalStateException: no row for The Matrix",
            report["message"]!!.jsonPrimitive.content,
        )
        assertTrue(report["stack"]!!.jsonArray.isNotEmpty())
        assertFalse(outside["incognito"]!!.jsonPrimitive.content.toBoolean())
        val inside = jvmReport("main", error, 5, incognito = true)
        assertNull(inside["report"]!!.jsonObject["message"], "the text may name what was playing")
        assertTrue(inside["incognito"]!!.jsonPrimitive.content.toBoolean())
    }
}
