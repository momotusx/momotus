package app.marquee.android

import android.os.Bundle
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

/** P1 stand-in screen: what the core module sees. Replaced by the real shell in P3. */
class MainActivity : ComponentActivity() {
    private lateinit var diagnostics: Diagnostics

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val app = application as MarqueeApp
        diagnostics = Diagnostics(app, app.session)
        diagnostics.start()
        setContent { DiagnosticsScreen(diagnostics) }
    }

    override fun onDestroy() {
        diagnostics.stop()
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
