plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "app.marquee.core"
    compileSdk = 37
    ndkVersion = "27.1.12297006"

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
        ndk { abiFilters += "arm64-v8a" }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        // UniFFI Kotlin, written by scripts/build-core.sh.
        getByName("main").kotlin.directories.add("src/main/generated")
    }

    lint {
        // Deliberate: arm64 first (plans/m6-android.md D10), as the app.
        disable += "ChromeOsAbiSupport"
    }
}

dependencies {
    // The aar variant carries libjnidispatch.so for Android.
    api(libs.jna) { artifact { type = "aar" } }
    api(libs.kotlinx.coroutines.android)
    api(libs.kotlinx.serialization.json)
    api(platform(libs.compose.bom))
    api(libs.compose.runtime)
    // Route keys implement NavKey; the runtime artifact has no UI.
    api(libs.androidx.navigation3.runtime)
    // The generated bindings use androidx.annotation.RequiresApi (android = true).
    implementation(libs.androidx.core.ktx)
    implementation(libs.rustls.platform.verifier)

    testImplementation(libs.junit)
    testImplementation(libs.kotlin.test.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}
