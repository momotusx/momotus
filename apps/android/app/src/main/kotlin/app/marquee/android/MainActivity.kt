package app.marquee.android

import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** P0 spike screen: a log of what the bridge does. Replaced by the real shell in P3. */
class MainActivity : ComponentActivity() {
    private val lines: SnapshotStateList<String> = mutableStateListOf()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            Column(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(24.dp),
            ) {
                for (line in lines) {
                    BasicText(line, style = TextStyle(color = Color(0xFFF6EFE6), fontSize = 14.sp))
                }
            }
        }
        val app = application as MarqueeApp
        Thread({ Spike(app) { log(it) }.run() }, "marquee-spike").start()
    }

    private fun log(line: String) {
        Log.i(MarqueeApp.TAG, line)
        // The emulator's log buffer rolls over quickly; keep the results readable with run-as.
        filesDir.resolve("p0-spike.txt").appendText(line + "\n")
        runOnUiThread { lines.add(line) }
    }
}
