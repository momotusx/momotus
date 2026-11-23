// AGP 9 compiles Kotlin itself (built-in Kotlin) and bundles an older Kotlin Gradle plugin.
// This classpath entry raises it to the version the Compose compiler and serialization
// plugins use. Keep it equal to `kotlin` in gradle/libs.versions.toml.
buildscript {
    repositories {
        google()
        // As in settings.gradle.kts: Google's mirror of Maven Central, then Central.
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
    }
    dependencies {
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.21")
    }
}

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.android.test) apply false
    alias(libs.plugins.androidx.baselineprofile) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.spotless)
}

// Robolectric's Android runtime is a Gradle dependency, so it comes from Gradle's cache like the rest. Left to
// itself Robolectric downloads it from Maven Central inside the first test of every run, and one failed download
// (a 404 there) failed every Robolectric test of the run (CI, 2026-11-20). Offline mode reads the jar from the
// cache folder Gradle keeps it in.
val robolectricRuntime: Configuration by configurations.creating {
    isCanBeConsumed = false
    isTransitive = false
}
dependencies { robolectricRuntime(libs.robolectric.android.all) }
subprojects {
    tasks.withType<Test>().configureEach {
        val runtime = robolectricRuntime.incoming.files
        inputs.files(runtime).withPropertyName("robolectricRuntime").withPathSensitivity(PathSensitivity.NAME_ONLY)
        jvmArgumentProviders.add(
            CommandLineArgumentProvider {
                listOf("-Drobolectric.offline=true", "-Drobolectric.dependency.dir=${runtime.singleFile.parent}")
            },
        )
    }
}

// Format and lint: ktlint through Spotless. Generated sources are left alone. ktlint's no-unused-imports
// does not run here unless it is named: without this line spotlessCheck passed 19 files with dead imports.
val ktlintRules = mapOf("ktlint_standard_no-unused-imports" to "enabled")
spotless {
    kotlin {
        target("*/src/**/*.kt")
        targetExclude("**/generated/**", "**/model/Generated.kt", "**/design/Tokens.kt", "**/design/Icons.kt")
        ktlint("1.8.0").editorConfigOverride(ktlintRules)
    }
    kotlinGradle {
        target("*.gradle.kts", "*/*.gradle.kts")
        ktlint("1.8.0").editorConfigOverride(ktlintRules)
    }
}
