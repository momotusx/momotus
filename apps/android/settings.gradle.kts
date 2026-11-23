pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        // Maven Central through Google's mirror first: Central rate-limits CI (a 429, then a 404, failed builds whose
        // Gradle cache had missed). Central itself only answers what the mirror does not have.
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        // The 10 KB JVM component of rustls-platform-verifier, vendored so builds are
        // offline and pinned to the version in Cargo.lock. First, so no remote is asked for it.
        maven {
            url = uri("vendor/maven")
            content { includeGroup("org.rustls") }
        }
        google()
        // Maven Central through Google's mirror first: Central rate-limits CI (a 429, then a 404, failed builds whose
        // Gradle cache had missed). Central itself only answers what the mirror does not have.
        maven("https://maven-central.storage-download.googleapis.com/maven2/")
        mavenCentral()
    }
}

rootProject.name = "marquee-android"

include(":app", ":core", ":design", ":player", ":ui-phone", ":ui-tv", ":macrobenchmark")
