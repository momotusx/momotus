// AGP 9 compiles Kotlin itself (built-in Kotlin) and bundles an older Kotlin Gradle plugin.
// This classpath entry raises it to the version the Compose compiler and serialization
// plugins use. Keep it equal to `kotlin` in gradle/libs.versions.toml.
buildscript {
    repositories {
        google()
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
