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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.marquee.core.client.CoreLog
import app.marquee.design.Form
import app.marquee.design.Tokens
import kotlinx.coroutines.launch

/**
 * Debug builds only: launch extras open the design gallery (`screen=gallery`) or the diagnostics
 * screen (`screen=diagnostics`), with `form=tv|phone` and `theme=dark|light`. Release builds have
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
     */
    fun launched(activity: ComponentActivity) {
        val url = activity.intent.getStringExtra("install_addon") ?: return
        val session = (activity.application as MarqueeApp).session
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
