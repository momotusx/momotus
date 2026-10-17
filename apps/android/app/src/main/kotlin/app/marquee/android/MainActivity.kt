package app.marquee.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
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

/**
 * Stand-in shell until P3. Debug extras: `screen=gallery form=tv|phone theme=dark|light` shows the
 * design gallery; without them the diagnostics screen shows what the core module sees.
 */
class MainActivity : ComponentActivity() {
    private var diagnostics: Diagnostics? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (intent.getStringExtra("screen") == "gallery") {
            val form = if (intent.getStringExtra("form") == "tv") Form.Tv else Form.Phone
            val dark = intent.getStringExtra("theme") != "light"
            setContent { GalleryHost(form, dark) }
            return
        }
        val app = application as MarqueeApp
        val diag = Diagnostics(app, app.session).also { it.start() }
        diagnostics = diag
        setContent { DiagnosticsScreen(diag) }
    }

    override fun onDestroy() {
        diagnostics?.stop()
        super.onDestroy()
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
            BasicText(line, style = TextStyle(color = Color(0xFFF6EFE6), fontSize = 13.sp))
        }
    }
}
