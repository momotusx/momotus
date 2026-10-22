import com.android.build.api.variant.BuildConfigField
import com.android.build.api.variant.FilterConfiguration

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.androidx.baselineprofile)
}

/** The last digit of a split APK's versionCode, by its ABI. */
val abiCodes = mapOf("armeabi-v7a" to 1, "arm64-v8a" to 2)

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
        // Accounts (plans/accounts.md 4.4, section 9): the hosted sync service, the link host (the passkeys' relying
        // party and the App Link host for /pair) and the Google web client id Credential Manager asks for. Each is
        // the owner's (gradle properties marquee.syncServer, marquee.linkHost, marquee.googleWebClientId); unset,
        // the first run shows today's doors, no passkey is offered and Google goes through the relay's own flow.
        buildConfigField("String", "SYNC_SERVER", "\"${property("marquee.syncServer")}\"")
        buildConfigField("String", "LINK_HOST", "\"${property("marquee.linkHost")}\"")
        buildConfigField("String", "GOOGLE_WEB_CLIENT_ID", "\"${property("marquee.googleWebClientId")}\"")
        // Which core this build packages, as scripts/build-core.sh wrote it: "own" (the torrent engine on arm64-v8a)
        // or "store" (none, STORE=1), "none" before the script has run; and the ABIs whose core carries the engine.
        val coreBuild =
            rootProject
                .file("core/core-build.properties")
                .takeIf { it.isFile }
                ?.readLines()
                .orEmpty()
                .associate { it.substringBefore('=') to it.substringAfter('=', "") }
        buildConfigField("String", "CORE_BUILD", "\"${coreBuild["kind"] ?: "none"}\"")
        buildConfigField("String", "CORE_ENGINE_ABIS", "\"${coreBuild["engine_abis"].orEmpty()}\"")
        // A host that never resolves (RFC 2606) stands in while there is none, so the App Link filter is valid.
        manifestPlaceholders["linkHost"] = property("marquee.linkHost").toString().ifEmpty { "link.marquee.invalid" }
    }

    // One APK per ABI, so a device installs only its own core library (a TV stick has little room):
    // arm64-v8a for phones, armeabi-v7a for TV sticks with a 32-bit userland such as the Fire TV Stick 4K
    // (plans/m6-android.md D10).
    splits {
        abi {
            // Off for a bundle build: AGP refuses several APK outputs there (issuetracker 402800800), and a
            // bundle splits by ABI itself. It is off for the whole invocation, so `assembleRelease bundleRelease`
            // together make one APK with both ABIs: run them apart. The bundle keeps the base versionCode (1)
            // while the APKs are 11 and 12; settle one scheme before a store upload.
            isEnable = gradle.startParameter.taskNames.none { it.contains("bundle", ignoreCase = true) }
            reset()
            include("arm64-v8a", "armeabi-v7a")
            isUniversalApk = false
        }
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
        jniLibs {
            useLegacyPackaging = false
            // The ABIs Marquee ships (D10). splits.abi shapes the APKs only, so a bundle would otherwise carry
            // the x86 libraries of JNA and graphics-path, and a store would offer x86 devices a build whose
            // core library is missing.
            excludes += listOf("**/x86/**", "**/x86_64/**")
        }
    }

    lint {
        // Deliberate: arm64 and armeabi-v7a, no x86 (plans/m6-android.md D10), targetSdk 36 until 37 is tested,
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
        // Their own package beside a device's Marquee: a benchmark run installs and uninstalls it, never the viewer's
        // app or its data.
        if (benchmark) variant.applicationId.set("app.marquee.android.benchmark")
        // One versionCode per ABI APK, so two APKs of one release never share one (arm64 above armeabi-v7a,
        // so a device that runs both takes arm64).
        variant.outputs.forEach { output ->
            val abi = output.filters.find { it.filterType == FilterConfiguration.FilterType.ABI }?.identifier
            val digit = abiCodes[abi] ?: return@forEach
            output.versionCode.set(output.versionCode.get() * 10 + digit)
        }
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
    // Accounts: Google's ID token and the account passkey in one Credential Manager sheet, and the relay's
    // brokered sign-in in an Auth Tab where Chrome has one, else a Custom Tab (plans/accounts.md 4.4).
    implementation(libs.androidx.credentials)
    implementation(libs.androidx.credentials.play.services.auth)
    implementation(libs.googleid)
    implementation(libs.androidx.browser)

    testImplementation(libs.junit)
    testImplementation(libs.kotlin.test.junit)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    debugImplementation(libs.compose.ui.test.manifest)
}
