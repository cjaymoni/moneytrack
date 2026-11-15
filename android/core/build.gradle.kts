// The Rust, and the bindings UniFFI writes from it. No screens in here.

plugins {
    id("com.android.library")
}

android {
    namespace = "gh.moneytrack.core"
    compileSdk = 37

    defaultConfig {
        minSdk = 26
        testOptions.targetSdk = 36
        consumerProguardFiles("consumer-rules.pro")
    }

    // `tools/android-libs.sh` builds the Rust for each ABI and writes the result here.
    // Checked out of source control: it is a build artifact, not a vendored binary.
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")
    sourceSets["main"].kotlin.srcDirs("src/main/kotlin")
    sourceSets["test"].kotlin.srcDirs("src/test/kotlin")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // UniFFI's Kotlin bindings reach the library through JNA. The `@aar` is the Android
    // packaging of it, which carries the native stubs for each ABI.
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")

    // Named outright rather than through `kotlin("test")`: that helper comes from the
    // Kotlin Gradle plugin, which AGP 9 replaces, and without it the dependency resolves to
    // the variant with no JUnit annotations in it.
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit:2.4.21")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.11.0")
    // The desktop build of JNA, for the unit tests: they run on the JVM against the
    // library built for this laptop, which is what makes the round trip testable in a
    // second rather than only on a handset.
    testImplementation("net.java.dev.jna:jna:5.19.1")
}

// The unit tests load the library through JNA, out of the workspace's own target
// directory. Which directory depends on the JDK: Gradle's test worker is whatever JVM is
// installed, and an x86_64 JDK running under Rosetta on an Apple silicon Mac needs an
// x86_64 library. `tools/android-libs.sh` builds the matching one; both candidates are on
// the path so neither arrangement needs a flag.
tasks.withType<Test>().configureEach {
    val target = when (System.getProperty("os.arch")) {
        "aarch64" -> if (System.getProperty("os.name").startsWith("Mac")) {
            "aarch64-apple-darwin"
        } else {
            "aarch64-unknown-linux-gnu"
        }
        "x86_64", "amd64" -> if (System.getProperty("os.name").startsWith("Mac")) {
            "x86_64-apple-darwin"
        } else {
            "x86_64-unknown-linux-gnu"
        }
        else -> null
    }
    val candidates = listOfNotNull(target?.let { "../target/$it/debug" }, "../target/debug")
    systemProperty(
        "jna.library.path",
        candidates.joinToString(File.pathSeparator) {
            rootProject.layout.projectDirectory.dir(it).asFile.absolutePath
        }
    )
}
