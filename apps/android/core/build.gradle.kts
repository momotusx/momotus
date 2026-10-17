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
}

dependencies {
    // The aar variant carries libjnidispatch.so for Android.
    api("${libs.jna.get().module}:${libs.versions.jna.get()}@aar")
    api(libs.kotlinx.coroutines.android)
    api(libs.kotlinx.serialization.json)
    api(platform(libs.compose.bom))
    api(libs.compose.runtime)
    // The generated bindings use androidx.annotation.RequiresApi (android = true).
    implementation(libs.androidx.core.ktx)
    implementation(libs.rustls.platform.verifier)
}
