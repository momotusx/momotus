plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "app.marquee.player"
    compileSdk = 37

    defaultConfig {
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
        unitTests.all {
            // Robolectric's Android 16 sets up shared memory through JDK internals.
            it.jvmArgs("--add-opens=java.base/jdk.internal.access=ALL-UNNAMED")
            // ManifestCorpusTest reads the core's golden playlists: a change reruns it.
            it.inputs.dir("../../../crates/mq-server/tests/corpus")
        }
    }
}

dependencies {
    api(project(":core"))
    api(project(":design"))
    api(libs.media3.exoplayer)
    api(libs.media3.ui.compose)
    implementation(libs.media3.ui)
    implementation(libs.media3.exoplayer.hls)
    implementation(libs.media3.exoplayer.dash)
    implementation(libs.media3.session)
    implementation(libs.androidx.core.ktx)
    // The player's lifecycle effects (PlayerLifecycle).
    implementation(libs.androidx.lifecycle.runtime.compose)

    testImplementation(libs.junit)
    testImplementation(testFixtures(project(":core")))
    testImplementation(libs.kotlin.test.junit)
    // Media3's playlist parsers take android.net.Uri (ManifestCorpusTest).
    testImplementation(libs.robolectric)
}
