// Plugin versions live here and nowhere else, so the two modules cannot drift apart.
//
// There is no `kotlin.android` plugin: AGP 9 compiles Kotlin itself, and declaring the old
// plugin beside it is an error rather than a duplicate.
plugins {
    id("com.android.application") version "9.5.1" apply false
    id("com.android.library") version "9.5.1" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
}
