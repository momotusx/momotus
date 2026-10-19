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
            // A live region speaks a change, not a view that comes with its text: one that just came
            // (the sign-in code) gets its words a moment after it is attached. The words may change
            // meanwhile (an add that was refused at once): the late write takes the newest ones.
            update = { view ->
                view.tag = spoken
                if (view.isAttachedToWindow) {
                    view.text = spoken
                } else {
                    view.postDelayed({ (view.tag as? String)?.let { if (view.text != it) view.text = it } }, 300)
                }
            },
            modifier = Modifier.matchParentSize(),
        )
        Box(Modifier.clearAndSetSemantics {}) { content() }
    }
}
