plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "app.marquee.ui.tv"
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
        // Compose UI tests run on Robolectric (the Versions screen).
        unitTests.isIncludeAndroidResources = true
        unitTests.all {
            // Robolectric and Compose outgrow Gradle's 512 MB default test heap; near it the collector stalls the JVM
            // for minutes and a test's wait runs out (CI, 2026-11-14).
            it.maxHeapSize = "2g"
            // Robolectric's Android 16 sets up shared memory through JDK internals.
            it.jvmArgs("--add-opens=java.base/jdk.internal.access=ALL-UNNAMED")
        }
    }
}

dependencies {
    api(project(":core"))
    api(project(":design"))
    api(project(":player"))
    implementation(libs.androidx.activity.compose)
    // Back from Settings asks again on the entry's resume (LifecycleResumeEffect).
    implementation(libs.androidx.lifecycle.runtime.compose)

    testImplementation(libs.junit)
    testImplementation(testFixtures(project(":core")))
    testImplementation(libs.kotlin.test.junit)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    // The subtitle view the cue test reads its cues' place from.
    testImplementation(libs.media3.ui)
    debugImplementation(libs.compose.ui.test.manifest)
}
