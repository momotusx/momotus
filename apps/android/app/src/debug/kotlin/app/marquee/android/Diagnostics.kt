package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.BridgeMetrics
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.RepeatingReplay
import app.marquee.core.client.Trace
import app.marquee.core.model.RowState
import app.marquee.core.secrets.KeystoreKeySource
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreSession
import app.marquee.core.views.HomeView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.io.File
import java.util.concurrent.Executors

/**
 * What the core module does, shown on screen and written to `files/p1-diag.txt`: core open time,
 * a live Home view, the secret store round trip on the real Keystore, and the bridge benchmark
 * (a recorded Home trace replayed through the real client, debug builds only).
 */
class Diagnostics(
    private val app: Application,
    private val session: CoreSession,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val log = mutableStateListOf<String>()
    private var home: HomeView? by mutableStateOf(null)
    private val report = File(app.filesDir, "p1-diag.txt")

    fun start() {
        report.delete()
        scope.launch { run() }
    }

    fun stop() {
        home?.close()
        scope.cancel()
    }

    fun lines(): List<String> {
        val live = mutableListOf<String>()
        home?.let { view ->
            live += "home: status=${view.status}, ${view.rows.size} rows, hero=${view.hero?.tile?.name}"
            for (row in view.rows) {
                val state = if (row.state is RowState.Ready) "ready" else row.state::class.simpleName
                live += "  ${row.title} (${row.source}): $state, ${row.tiles.size} tiles"
            }
        }
        return log + live
    }

    private fun add(line: String) {
        log.add(line)
        report.appendText(line + "\n")
    }

    private suspend fun run() {
        try {
            session.ready.await()
            add("core ${session.version()} open in ${session.openMillis} ms")
            val started = System.nanoTime()
            runCatching { session.ops.installAddon(CINEMETA) }
                .onSuccess { add("installAddon ${it.name}: ${ms(started)} ms") }
                .onFailure { add("installAddon: ${it.message}") }
            home = session.home()
            delay(8_000)
            add("bridge, live Home: emit to applied ${session.metrics.emitToApplied().toJson()}")
            add("bridge, live Home: batches ${session.metrics.batches().toJson()}")
            secretSelfTest()
            if (BuildConfigDebug.enabled(app)) bridgeBench()
            add("done")
        } catch (e: Exception) {
            add("diagnostics failed: $e")
        }
    }

    private fun secretSelfTest() {
        val file = File(app.noBackupFilesDir, "secrets-selftest.bin")
        val keys = KeystoreKeySource("marquee.selftest.v1")
        try {
            file.delete()
            val store = SecretStore(file, keys)
            store.update { it.copy(debridKeys = listOf("realdebrid" to "SELFTEST-KEY-0123456789")) }
            val sealed = file.readBytes()
            val plaintextOnDisk = String(sealed, Charsets.ISO_8859_1).contains("SELFTEST-KEY")
            val back = SecretStore(file, keys).load().debridKeys
            val tampered = sealed.copyOf().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() }
            file.writeBytes(tampered)
            val reset = SecretStore(file, keys)
            val afterTamper = reset.load()
            add(
                "secret store on the Keystore: sealed ${sealed.size} bytes, plaintext on disk=$plaintextOnDisk, " +
                    "round trip=${back == listOf("realdebrid" to "SELFTEST-KEY-0123456789")}, " +
                    "tampered blob -> empty=${afterTamper.debridKeys.isEmpty()} health=${reset.health}",
            )
        } catch (e: Exception) {
            add("secret store self-test failed: $e")
        } finally {
            file.delete()
            keys.reset()
        }
    }

    /** Replays a recorded Home trace through the real client 300 times and reports the timings. */
    private suspend fun bridgeBench() {
        val text =
            app.assets
                .open("traces/home.jsonl")
                .bufferedReader()
                .use { it.readText() }
        val trace = Trace.parse(text)
        val metrics = BridgeMetrics()
        val executor = Executors.newSingleThreadExecutor { Thread(it, "marquee-bench-state") }
        val client = CoreClient(executor.asCoroutineDispatcher(), CoreJson, metrics)
        try {
            client.attach(RepeatingReplay(trace) { client.onMessage(it) })
            // The first runs are the JIT warming up; only the 300 after them are reported.
            repeat(450) { i ->
                if (i == 150) {
                    delay(100)
                    metrics.reset()
                }
                val view = HomeView(client, scope).also { it.open() }
                view.opened.await()
                delay(4)
                view.close()
            }
            delay(200)
            val load = runCatching { File("/proc/loadavg").readText().trim() }.getOrDefault("?")
            add(
                "bridge, replayed Home trace x300 after warm-up (load $load): emit to applied ${metrics.emitToApplied().toJson()}",
            )
            add("bridge, replayed Home trace x300 after warm-up: batches ${metrics.batches().toJson()}")
        } finally {
            client.close()
            executor.shutdown()
        }
    }

    private fun ms(from: Long) = (System.nanoTime() - from) / 1_000_000

    private companion object {
        const val CINEMETA = "https://v3-cinemeta.strem.io/manifest.json"
    }
}

/** The benchmark needs the recorded trace, which only debug builds ship as an asset. */
private object BuildConfigDebug {
    fun enabled(app: Application): Boolean = runCatching { app.assets.open("traces/home.jsonl").close() }.isSuccess
}
