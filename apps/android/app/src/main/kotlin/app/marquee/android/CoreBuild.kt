package app.marquee.android

import app.marquee.core.client.CoreLog

/**
 * Which core the build was made with (BuildConfig.CORE_BUILD: own or store, from scripts/build-core.sh) beside
 * what the core says it carries (`hasEngine()`). Every own build carries the torrent engine
 * (plans/play-every-version.md, Phase 0), so a mismatch is a mis-built core: the viewer sees the core's quiet
 * account state, as on a store build, and the owner gets [ABOUT_LINE] in Settings, About, a log line and, in a
 * debug build, a failed assertion. As on Apple (CoreBuild.swift).
 */
internal object CoreBuild {
    const val ABOUT_LINE = "This build's core has no torrent engine"

    /** An own build whose core has no engine; a store build has none on purpose. */
    fun missingEngine(
        field: String,
        hasEngine: Boolean,
    ): Boolean = field != "store" && !hasEngine

    /** Whether Settings, About shows [ABOUT_LINE]: set by [check] once the core has opened. */
    @Volatile
    var missing = false
        private set

    /** Once the core has opened: the log line and, in a debug build, the assertion. */
    fun check(
        field: String,
        hasEngine: Boolean,
        debug: Boolean,
    ) {
        missing = missingEngine(field, hasEngine)
        if (!missing) return
        CoreLog.e("core build $field: $ABOUT_LINE")
        check(!debug) { ABOUT_LINE }
    }
}
