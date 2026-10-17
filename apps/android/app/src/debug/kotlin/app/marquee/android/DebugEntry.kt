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
import app.marquee.design.Form
import app.marquee.design.Tokens

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
        dark: Boolean,
    ): Boolean {
        when (activity.intent.getStringExtra("screen")) {
            "gallery" -> {
                activity.setContent { GalleryHost(form, dark) }
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
