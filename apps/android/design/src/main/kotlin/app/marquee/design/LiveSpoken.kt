package app.marquee.design

import android.view.View
import android.widget.TextView
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.viewinterop.AndroidView

/**
 * [content] with [spoken] read out by TalkBack once each time it changes: a native live region under it,
 * since a Compose live region on a card reached TalkBack only as a change of its parent. [content] is drawn
 * over it and not read twice; it must hold no control.
 */
@Composable
fun LiveSpoken(
    spoken: String,
    content: @Composable () -> Unit,
) {
    Box {
        AndroidView(
            factory = { context ->
                TextView(context).apply {
                    accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
                    setTextColor(android.graphics.Color.TRANSPARENT)
                }
            },
            // Each change is written a moment later, the newest words of that moment. A line that just
            // appeared is attached by then (a live region speaks a change, not a view that comes with its
            // text), and words that change at once (an add refused at once: "Adding the addon", then the
            // warning) are written once: TalkBack drops a live region that changes again right after it spoke.
            update = { view ->
                view.tag = spoken
                view.postDelayed({ view.writeNewest() }, QUIET_MS)
            },
            modifier = Modifier.matchParentSize(),
        )
        Box(Modifier.clearAndSetSemantics {}) { content() }
    }
}

/** Writes the newest words; the same words again are no change. */
private fun TextView.writeNewest() {
    val newest = tag as? String ?: return
    if (text.toString() != newest) text = newest
}

/** How long a live line waits before its words are written, so a change at once is written once. */
private const val QUIET_MS = 400L
