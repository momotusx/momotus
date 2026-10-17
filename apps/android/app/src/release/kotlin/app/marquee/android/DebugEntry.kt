package app.marquee.android

import androidx.activity.ComponentActivity
import app.marquee.design.Form

/** The release build has no debug screens; see `src/debug` for the real one. */
object DebugEntry {
    @Suppress("UNUSED_PARAMETER")
    fun show(
        activity: ComponentActivity,
        form: Form,
        dark: Boolean,
    ): Boolean = false

    fun destroy() = Unit
}
