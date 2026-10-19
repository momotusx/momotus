pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        google()
        mavenCentral()
        // The 10 KB JVM component of rustls-platform-verifier, vendored so builds are
        // offline and pinned to the version in Cargo.lock.
        maven {
            url = uri("vendor/maven")
            content { includeGroup("org.rustls") }
        }
    }
}

rootProject.name = "marquee-android"

include(":app", ":core", ":design", ":player", ":ui-phone", ":ui-tv", ":macrobenchmark")
