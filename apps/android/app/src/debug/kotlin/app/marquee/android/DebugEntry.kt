package app.marquee.android

import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreLog
import app.marquee.design.Form
import app.marquee.design.Tokens
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.jsonObject

/**
 * Debug builds only: launch extras open the design gallery (`screen=gallery`) or the diagnostics
 * screen (`screen=diagnostics`), or measure Find addons (`screen=bench-find-addons`), with `form=tv|phone` and `theme=dark|light`. Release builds have
 * a stub in `src/release`, so none of this ships.
 */
object DebugEntry {
    private var diagnostics: Diagnostics? = null

    /** Shows a debug screen when the launch asks for one; false means start the app as usual. */
    fun show(
        activity: ComponentActivity,
        form: Form,
    ): Boolean {
        when (activity.intent.getStringExtra("screen")) {
            "gallery" -> {
                activity.setContent { GalleryHost(form) }
            }

            "bench-find-addons" -> {
                val lines = FindAddonsBench.run()
                activity.setContent { BasicText(lines.joinToString("\n"), Modifier.padding(24.dp)) }
            }

            "diagnostics" -> {
                val app = activity.application as MarqueeApp
                val diag = Diagnostics(app, app.session).also { it.start() }
                diagnostics = diag
                activity.setContent { DiagnosticsScreen(diag) }
            }

            else -> {
                return false
            }
        }
        return true
    }

    /**
     * `install_addon=<manifest url>` installs an addon at launch: the local test addon
     * (scripts/sintel-addon.ts over `adb reverse`) before there is a screen to do it from.
     * `kill_core_after=<seconds>` hands the app the message a panicking core sends, that long
     * after launch, to walk what the app does across a core death and its restart.
     * `legacy_debrid=<service>:<key>` stores a debrid key the way builds before records did; the next
     * core open moves it to a record. `prefs_patch=<json>` merges a `setPrefs` patch at launch: what the
     * picks taught (`{"learned":{"fhd":{"size":2}}}`), so a walk reaches a learned note in one pick.
     */
    fun launched(activity: ComponentActivity) {
        val session = (activity.application as MarqueeApp).session
        val legacy = activity.intent.getStringExtra("legacy_debrid")?.split(":", limit = 2)
        if (legacy?.size == 2) {
            val (service, key) = legacy
            session.secrets.update { s ->
                s.copy(
                    debridKeys =
                        s.debridKeys.filter { it.first != service } + (service to key),
                )
            }
            CoreLog.d("debug: a legacy $service key stored")
        }
        val killAfter = activity.intent.getIntExtra("kill_core_after", 0)
        if (killAfter > 0) {
            session.scope.launch {
                delay(killAfter * 1_000L)
                CoreLog.d("debug: the core dies")
                session.client.onMessage("""{"core":"died","reason":"debug: a simulated panic"}""")
            }
        }
        activity.intent.getStringExtra("prefs_patch")?.let { patch ->
            session.scope.launch {
                runCatching { session.ops.setPrefs(CoreJson.parseToJsonElement(patch).jsonObject) }
                    .onSuccess { CoreLog.d("debug: prefs patched") }
                    .onFailure { CoreLog.w("debug: prefs patch failed: ${it.message}") }
            }
        }
        val url = activity.intent.getStringExtra("install_addon") ?: return
        session.scope.launch {
            runCatching { session.ops.installAddon(url) }
                .onSuccess { CoreLog.d("debug: installed addon ${it.name}") }
                .onFailure { CoreLog.w("debug: addon install failed: ${it.message}") }
        }
    }

    fun destroy() {
        diagnostics?.stop()
        diagnostics = null
    }

    /**
     * Local fakes for emulator walks: Trakt's base URL in `files/dev-trakt`, Find addons' source manifest
     * in `files/dev-cinemeta`, Stremio's account and link services in `files/dev-stremio` (scripts/fake-stremio.ts,
     * Import from Stremio without an account), written with `adb shell run-as`, each an http URL on this
     * device, read only by a core library built with debug assertions (`build-core.sh dev`), which only debug
     * builds package. No file, no change.
     */
    fun devFakes(context: android.content.Context) {
        val fakes =
            listOf(
                "dev-trakt" to "MQ_DEV_TRAKT",
                "dev-cinemeta" to "MQ_DEV_CINEMETA",
                "dev-stremio" to "MQ_DEV_STREMIO",
            )
        for ((file, name) in fakes) {
            val url =
                java.io
                    .File(context.filesDir, file)
                    .takeIf { it.exists() }
                    ?.readText()
                    ?.trim()
                    ?.ifEmpty { null } ?: continue
            android.system.Os.setenv(name, url, true)
            CoreLog.w("debug: $name is $url (the core takes it only as an http URL on this device)")
        }
    }
}

@Composable
private fun DiagnosticsScreen(diagnostics: Diagnostics) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
    ) {
        for (line in diagnostics.lines()) {
            BasicText(line, style = TextStyle(color = Tokens.dark.text, fontSize = 13.sp))
        }
    }
}
