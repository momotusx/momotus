plugins {
    alias(libs.plugins.android.test)
    alias(libs.plugins.androidx.baselineprofile)
}

// Startup, Home scroll and playback start on a device or emulator, and the baseline profile generator.
// Benchmarks: ANDROID_SERIAL=<serial> ./gradlew :macrobenchmark:connectedBenchmarkReleaseAndroidTest
// Profile: ANDROID_SERIAL=<serial> ./gradlew :app:generateBaselineProfile (the generator alone, see below)
android {
    namespace = "app.marquee.macrobenchmark"
    compileSdk = 37

    defaultConfig {
        minSdk = 28
        targetSdk = 36
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // An emulator's numbers are labeled EMULATOR_ in the results instead of refused; they show
        // regressions, not what a TV stick does (plans/m6-android.md 11.4).
        testInstrumentationRunnerArguments["androidx.benchmark.suppressErrors"] = "EMULATOR"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    targetProjectPath = ":app"
}

// The connected emulator or device records the profile, with no managed device.
baselineProfile {
    useConnectedDevices = true
}

// The profile's run is the generator alone: over the whole module (the frame and startup tests skipped) the
// Pixel's test results listed no profile, and the plugin emptied the app's (2026-10-22, app/build.gradle.kts).
// Only the profile's variant: the benchmarks keep every test.
androidComponents {
    onVariants(selector().withBuildType("nonMinifiedRelease")) {
        it.instrumentationRunnerArguments.put("class", "app.marquee.macrobenchmark.BaselineProfileGenerator")
    }
}

dependencies {
    implementation(libs.androidx.benchmark.macro.junit4)
    implementation(libs.androidx.test.ext.junit)
    implementation(libs.androidx.test.uiautomator)
}
