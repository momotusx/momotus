plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.serialization)
    // RecordView.current(): a view's snapshot as a screen reads it.
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "app.marquee.core"
    compileSdk = 37
    ndkVersion = "27.1.12297006"

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
        // Phones are arm64; many TV sticks run a 32-bit userland (plans/m6-android.md D10).
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a") }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        // UniFFI Kotlin, written by scripts/build-core.sh.
        getByName("main").kotlin.directories.add("src/main/generated")
    }

    testOptions {
        unitTests.all {
            // Robolectric's Android 16 sets up shared memory through JDK internals (CoreStartTest).
            it.jvmArgs("--add-opens=java.base/jdk.internal.access=ALL-UNNAMED")
        }
    }

    lint {
        // Deliberate: arm64 and armeabi-v7a, no x86 (plans/m6-android.md D10), as the app.
        disable += "ChromeOsAbiSupport"
    }
}

// A release or bundle build needs the core library of every ABI it ships: a split without one installs and dies
// at load. scripts/build-core.sh writes them; debug builds may carry one ABI for a walk on one device.
val coreLibs =
    listOf("arm64-v8a", "armeabi-v7a").map {
        layout.projectDirectory.file("src/main/jniLibs/$it/libmarquee_core.so").asFile
    }
val checkCoreLibs by tasks.registering {
    val libs = coreLibs
    doLast {
        val missing = libs.filterNot { it.isFile }
        if (missing.isNotEmpty()) {
            throw GradleException(
                "the core library is missing for ${missing.joinToString {
                    it.parentFile.name
                }}: run scripts/build-core.sh",
            )
        }
    }
}
tasks.matching { it.name == "preReleaseBuild" }.configureEach { dependsOn(checkCoreLibs) }

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
    // A real CoreSession needs a Context (CoreStartTest).
    testImplementation(libs.robolectric)
}
