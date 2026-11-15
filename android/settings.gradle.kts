pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "moneytrack"

// Two modules, and the split is the architecture in miniature: `core` is the Rust and the
// generated bindings, with no Android UI in it at all, and `app` is screens. Anything that
// decides what a number is belongs below `core`, in Rust.
include(":core")
include(":app")
