package app.marquee.android

import android.content.Context
import android.os.Build
import app.marquee.core.reports.CrashReports
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Error reports on Android (plans/error-reports.md 5.1, 6.2): the JVM handler from the app's first line, then on each
 * core this process opens, `reportContext` (who the app is and the relay) and the crashes read after the fact. Only
 * builds with [BuildConfig.REPORTS] (release builds; debug and walk builds with -Pmarquee.reports=true) send it; the
 * core sends nothing to an open that never got it.
 */
object Reports {
    lateinit var crashes: CrashReports
        private set

    /** [start] ran: the handler and the switch's cached copy exist. */
    val started: Boolean
        get() = ::crashes.isInitialized

    fun start(
        context: Context,
        session: CoreSession,
        incognito: () -> Boolean,
    ) {
        crashes = CrashReports(context).also { it.installHandler(incognito) }
        if (!BuildConfig.REPORTS) return
        val form = FormFactor.detect(context, null)
        session.onAttached = {
            session.client.post("reportContext", contextFields(form, session.version()))
            session.scope.launch { crashes.deliver(session.client) }
        }
    }

    /** The `reportContext` fields: versions and the device model in the relay's charset, never an identifier. */
    fun contextFields(
        form: Form,
        coreVersion: String?,
    ) = mapOf(
        "device" to
            buildJsonObject {
                put("app", "android")
                put(
                    "channel",
                    if (BuildConfig.WALK) {
                        "walk"
                    } else if (BuildConfig.DEBUG) {
                        "debug"
                    } else {
                        "release"
                    },
                )
                put("version", plain(BuildConfig.VERSION_NAME, 32).ifEmpty { "0" })
                put("core", plain(coreVersion.orEmpty(), 40))
                put("os", plain("Android ${Build.VERSION.RELEASE}", 40))
                put("model", plain("${Build.MANUFACTURER} ${Build.MODEL}", 60).ifEmpty { "Android" })
                put("abi", plain(Build.SUPPORTED_ABIS.firstOrNull().orEmpty(), 20))
                put("form", if (form == Form.Tv) "tv" else "phone")
                put("coreBuild", if (BuildConfig.CORE_BUILD == "store") "store" else "own")
            },
        "server" to JsonPrimitive(BuildConfig.SYNC_SERVER),
    )

    /** `text` with anything outside the relay's charset for device fields made a space, trimmed to [max]. */
    fun plain(
        text: String,
        max: Int,
    ): String =
        text
            .map { if ((it.isLetterOrDigit() && it.code < 128) || it in " ,._()+-") it else ' ' }
            .joinToString("")
            .trim()
            .take(max)
            .trim()
}
