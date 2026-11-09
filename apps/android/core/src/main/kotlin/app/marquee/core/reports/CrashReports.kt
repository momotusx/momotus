package app.marquee.core.reports

import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.content.Context
import android.os.Build
import androidx.annotation.RequiresApi
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreLog
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import java.io.File

/**
 * Android's side of error reports (plans/error-reports.md 6.2): crashes the core cannot see. A JVM crash is written
 * to a pending file as the process dies; the next launch hands each file, and the system's exit records of native
 * crashes, ANRs and foreground low-memory kills (API 30+), to the core through its `report` op. The core strips,
 * queues, scrubs and uploads; nothing here talks to the relay or scrubs.
 *
 * Off ([off], the switch's cached copy, written whenever the switch changes): the JVM handler writes nothing, the
 * pending files are deleted, exit records are read and dropped (their marker still moves on, so turning reports back
 * on never sends old crashes).
 */
class CrashReports(
    context: Context,
) {
    private val prefs = context.getSharedPreferences("reports", Context.MODE_PRIVATE)
    private val pending = File(context.noBackupFilesDir, "reports-pending")
    private val activities = context.getSystemService(ActivityManager::class.java)
    private val packageName = context.packageName

    /** The switch, as this app last heard it from the core. */
    var off: Boolean
        get() = prefs.getBoolean(OFF, false)
        set(value) {
            prefs.edit().putBoolean(OFF, value).apply()
            if (value) deletePending()
        }

    /**
     * The JVM's last word: a pending file (class, frames, thread; the message only outside Incognito), written
     * synchronously, then the handler that was there before, so the system's crash dialog and exit stay as they are.
     */
    fun installHandler(incognito: () -> Boolean) {
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching { if (!off) writePending(thread.name, error, incognito()) }
            previous?.uncaughtException(thread, error)
        }
    }

    private fun writePending(
        thread: String,
        error: Throwable,
        incognito: Boolean,
    ) {
        pending.mkdirs()
        if ((pending.list()?.size ?: 0) >= MAX_PENDING) return
        val file = File(pending, "jvm-${System.currentTimeMillis()}.json")
        file.writeText(jvmReport(thread, error, System.currentTimeMillis(), incognito).toString())
    }

    private fun deletePending() {
        pending.listFiles()?.forEach { it.delete() }
    }

    /**
     * After `reportContext`: every pending file and every new exit record to the core, each file deleted once the
     * core took it. Runs off the main thread.
     */
    suspend fun deliver(client: CoreClient) {
        val records = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) newExitRecords() else emptyList()
        if (off) {
            deletePending()
            return
        }
        for (file in pending.listFiles().orEmpty().sortedBy { it.name }) {
            val body =
                runCatching {
                    kotlinx.serialization.json.Json
                        .parseToJsonElement(
                            file.readText(),
                        ).jsonObject
                }.getOrNull()
            if (body != null) {
                runCatching { client.request("report", body) }.onFailure {
                    CoreLog.w("a crash report was not handed over: ${CoreLog.describe(it)}")
                    return
                }
            }
            file.delete()
        }
        for (record in records) {
            runCatching { client.request("report", record) }
        }
    }

    /** The exit records since the last read, as `report` fields; the marker moves on whatever the switch says. */
    @RequiresApi(Build.VERSION_CODES.R)
    private fun newExitRecords(): List<Map<String, JsonElement>> {
        val seen = prefs.getLong(EXIT_SEEN, 0L)
        val infos =
            runCatching {
                activities.getHistoricalProcessExitReasons(
                    packageName,
                    0,
                    16,
                )
            }.getOrDefault(emptyList())
        val newest = infos.maxOfOrNull { it.timestamp } ?: seen
        if (newest > seen) prefs.edit().putLong(EXIT_SEEN, newest).apply()
        val lowMemoryReported = runCatching { ActivityManager.isLowMemoryKillReportSupported() }.getOrDefault(true)
        return infos
            .filter { it.timestamp > seen && it.processName == packageName }
            .mapNotNull { info ->
                val record =
                    ExitRecord(
                        timestamp = info.timestamp,
                        reason = info.reason,
                        status = info.status,
                        importance = info.importance,
                        description = info.description,
                        pssKb = info.pss,
                        rssKb = info.rss,
                        trace =
                            if (info.reason == ApplicationExitInfo.REASON_ANR) {
                                runCatching { info.traceInputStream?.bufferedReader()?.use { it.readText() } }
                                    .getOrNull()
                            } else {
                                null
                            },
                    )
                exitReport(record, lowMemoryReported)?.let { mapOf("report" to it) }
            }
    }

    companion object {
        private const val OFF = "off"
        private const val EXIT_SEEN = "exitSeen"
        private const val MAX_PENDING = 30
    }
}

/** One system exit record, as [exitReport] reads it (a plain copy, so the rules test without a device). */
data class ExitRecord(
    val timestamp: Long,
    val reason: Int,
    val status: Int,
    val importance: Int,
    val description: String?,
    val pssKb: Long,
    val rssKb: Long,
    val trace: String?,
)

private const val MAX_FRAMES = 48
private const val SIGKILL = 9

/** A JVM crash as the pending file holds it: `report` and the app's own `incognito` word. */
fun jvmReport(
    thread: String,
    error: Throwable,
    now: Long,
    incognito: Boolean,
): JsonObject {
    var root = error
    while (root.cause != null && root.cause !== root) root = root.cause!!
    val report =
        buildJsonObject {
            put("kind", "crash")
            put("at", now)
            // In Incognito the text stays out of the file: it may name what was playing.
            if (!incognito) put("message", "${root.javaClass.name}: ${root.message.orEmpty()}".take(512))
            put("thread", thread.take(40))
            put("stack", JsonArray(root.stackTrace.take(MAX_FRAMES).map { JsonPrimitive(it.toString()) }))
        }
    return buildJsonObject {
        put("report", report)
        put("incognito", incognito)
    }
}

/**
 * An exit record as a report, or null when it is not one: native crashes (description and signal), ANRs (the main
 * thread's frames), and low-memory kills in the foreground. Where the device cannot report low-memory kills, a
 * foreground SIGKILL stands for one. JVM crashes are the handler's.
 */
fun exitReport(
    record: ExitRecord,
    lowMemoryReported: Boolean,
): JsonObject? {
    val foreground = record.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
    val kind =
        when {
            record.reason == ApplicationExitInfo.REASON_CRASH_NATIVE -> "crash"

            record.reason == ApplicationExitInfo.REASON_ANR -> "anr"

            record.reason == ApplicationExitInfo.REASON_LOW_MEMORY && foreground -> "oom"

            !lowMemoryReported && record.reason == ApplicationExitInfo.REASON_SIGNALED &&
                record.status == SIGKILL && foreground -> "oom"

            else -> return null
        }
    return buildJsonObject {
        put("kind", kind)
        put("at", record.timestamp)
        val message =
            when (kind) {
                "crash" -> {
                    "native crash, signal ${record.status}: ${record.description.orEmpty()}"
                }

                "anr" -> {
                    "not responding: ${record.description.orEmpty()}"
                }

                else -> {
                    "low memory kill in the foreground, last sampled " +
                        "pss ${record.pssKb / 1024} MB, rss ${record.rssKb / 1024} MB"
                }
            }
        put("message", message.take(512))
        if (kind == "anr") {
            put("thread", "main")
            put("stack", JsonArray(mainThreadFrames(record.trace.orEmpty()).map { JsonPrimitive(it) }))
        }
    }
}

/** The `"main"` thread's `at ...` lines from an ANR trace, without the `at `, at most [MAX_FRAMES]. */
fun mainThreadFrames(trace: String): List<String> {
    val lines = trace.lines()
    val start = lines.indexOfFirst { it.startsWith("\"main\"") }
    if (start < 0) return emptyList()
    return lines
        .drop(start + 1)
        .takeWhile { it.isNotBlank() }
        .map { it.trim() }
        .filter { it.startsWith("at ") }
        .map { it.removePrefix("at ") }
        .take(MAX_FRAMES)
}
