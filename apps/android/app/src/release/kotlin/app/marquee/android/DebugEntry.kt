package app.marquee.android

import androidx.activity.ComponentActivity
import app.marquee.design.Form

/** The release build has no debug screens; see `src/debug` for the real one. */
object DebugEntry {
    @Suppress("UNUSED_PARAMETER")
    fun show(
        activity: ComponentActivity,
        form: Form,
    ): Boolean = false

    fun launched(activity: ComponentActivity) = Unit

    fun destroy() = Unit

    @Suppress("UNUSED_PARAMETER")
    fun devFakes(context: android.content.Context) = Unit
}
