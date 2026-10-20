import com.android.build.api.variant.BuildConfigField

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.androidx.baselineprofile)
}

android {
    namespace = "app.marquee.android"
    compileSdk = 37
    ndkVersion = "27.1.12297006"

    defaultConfig {
        applicationId = "app.marquee.android"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Local release builds only; store signing is a release-process decision.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { buildConfig = true }

    testOptions {
        // Compose tests run on Robolectric with the app's resources and manifest (its intent filters).
        unitTests.isIncludeAndroidResources = true
        unitTests.all {
            // Robolectric's Android 16 sets up shared memory through JDK internals.
            // Old ISO codes ("iw", "in", "ji") as Android's Locale keeps them, so a test sees what a phone gives.
            it.jvmArgs("--add-opens=java.base/jdk.internal.access=ALL-UNNAMED", "-Djava.locale.useOldISOCodes=true")
        }
    }

    packaging {
        jniLibs { useLegacyPackaging = false }
    }

    lint {
        // Deliberate: arm64 first (plans/m6-android.md D10), targetSdk 36 until 37 is tested,
        // and the Gradle and AGP pins are decided in the plan.
        disable += listOf("ChromeOsAbiSupport", "OldTargetApi", "AndroidGradlePluginVersion", "GradleDependency")
    }

    sourceSets {
        // The recorded protocol traces double as the bridge benchmark input in debug builds.
        getByName("debug").assets.directories.add("../core/src/test/resources")
    }
}

// The baseline profile (plans/m6-android.md P9): :macrobenchmark records it on a connected device with
// `scripts/gw.sh :app:generateBaselineProfile`, never during a build, into src/main/generated/baselineProfiles.
// The plugin's benchmarkRelease (measured) and nonMinifiedRelease (recorded) are release builds, never shipped.
baselineProfile {
    automaticGenerationDuringBuild = false
    saveInSrc = true
    mergeIntoMain = true
}

// The plugin's release copies take the release source set (no debug screens or fakes), and say they are
// benchmark builds: only they and debug builds open a title from a launch extra.
androidComponents {
    onVariants { variant ->
        val benchmark = variant.buildType == "benchmarkRelease" || variant.buildType == "nonMinifiedRelease"
        variant.buildConfigFields?.put("BENCHMARK", BuildConfigField("boolean", "$benchmark", "a benchmark build"))
        if (benchmark) variant.sources.kotlin?.addStaticSourceDirectory("src/release/kotlin")
    }
}

dependencies {
    baselineProfile(project(":macrobenchmark"))
    implementation(project(":core"))
    implementation(project(":design"))
    implementation(project(":player"))
    implementation(project(":ui-phone"))
    implementation(project(":ui-tv"))
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.viewmodel.navigation3)
    implementation(libs.androidx.navigation3.ui)
    // Scanning a pairing code: CameraX frames, read by ZXing (already here for drawing codes).
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.zxing.core)
    // Installs the baseline profile (src/main/baseline-prof.txt) where the store does not.
    implementation(libs.androidx.profileinstaller)

    testImplementation(libs.junit)
    testImplementation(libs.kotlin.test.junit)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    debugImplementation(libs.compose.ui.test.manifest)
}
