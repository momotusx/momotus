plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
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

dependencies {
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

    testImplementation(libs.junit)
    testImplementation(libs.kotlin.test.junit)
}
