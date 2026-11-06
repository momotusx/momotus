package app.marquee.android

import app.marquee.core.client.CoreLog

/**
 * Which core the build was made with (BuildConfig.CORE_BUILD: own or store, and CORE_ENGINE_ABIS, the ABIs whose
 * core carries the engine, from scripts/build-core.sh) beside what the core says it carries (`hasEngine()`). Every
 * own build carries the torrent engine on arm64-v8a and armeabi-v7a (plans/play-every-version.md, Steps 0 and 0b), so
 * a mismatch is a mis-built core: the viewer sees the core's quiet
 * account state, as on a store build, and the owner gets [ABOUT_LINE] in Settings, About, a log line and, in a
 * debug build, a failed assertion. As on Apple (CoreBuild.swift).
 */
internal object CoreBuild {
    const val ABOUT_LINE = "This build's core has no torrent engine"

    /**
     * An own build whose core on this device's [abi] should carry the engine and has none; a store build has none on
     * purpose, and an ABI outside [engineAbis] (comma-separated) too.
     */
    fun missingEngine(
        field: String,
        engineAbis: String,
        abi: String,
        hasEngine: Boolean,
    ): Boolean = field != "store" && abi in engineAbis.split(',') && !hasEngine

    /** Whether Settings, About shows [ABOUT_LINE]: set by [check] once the core has opened. */
    @Volatile
    var missing = false
        private set

    /** Once the core has opened: the log line and, in a debug build, the assertion. */
    fun check(
        field: String,
        engineAbis: String,
        abi: String,
        hasEngine: Boolean,
        debug: Boolean,
    ) {
        missing = missingEngine(field, engineAbis, abi, hasEngine)
        if (!missing) return
        CoreLog.e("core build $field: $ABOUT_LINE")
        check(!debug) { ABOUT_LINE }
    }
}
