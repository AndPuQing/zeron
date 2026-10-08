plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose.compiler)
}

// The Rust mobile core (crates/mobile) — the library the iOS app links —
// built for Android with its generated Kotlin bindings. `-PzeronSkipCore`
// reuses the last build while iterating on Kotlin only.
val NDK_VERSION = "29.0.14206865"
val repoRoot = rootProject.projectDir.resolve("../..").canonicalFile
val forkVersion = Regex("(?m)^version = \"(\\d+\\.\\d+\\.\\d+)\"$")
    .find(repoRoot.resolve("Cargo.toml").readText())?.groupValues?.get(1)
    ?: error("Missing numeric workspace version in Cargo.toml")
val versionParts = forkVersion.split('.').map(String::toInt)
require(versionParts[1] < 1000 && versionParts[2] < 1000) {
    "Android versionCode requires minor and patch versions below 1000"
}
val forkVersionCode = versionParts[0].toLong() * 1_000_000 + versionParts[1] * 1000 + versionParts[2]
require(forkVersionCode in 1..2_100_000_000L) { "Android versionCode is out of range" }
val coreOut = repoRoot.resolve("target/android-core")
val iconsOut = layout.buildDirectory.dir("generated/zeron-icons")
val skipCore = providers.gradleProperty("zeronSkipCore").isPresent
val releaseSigning = listOf(
    "ZERUN_ANDROID_KEYSTORE",
    "ZERUN_ANDROID_STORE_PASSWORD",
    "ZERUN_ANDROID_KEY_ALIAS",
    "ZERUN_ANDROID_KEY_PASSWORD",
).associateWith { providers.environmentVariable(it).orNull }
val missingSigning = releaseSigning.filterValues { it.isNullOrBlank() }.keys
val releaseKeystore = releaseSigning["ZERUN_ANDROID_KEYSTORE"]?.let(::file)

val validateReleaseSigning by tasks.registering {
    description = "Requires the production signing credentials for release builds."
    doLast {
        check(missingSigning.isEmpty()) {
            "Release signing requires: ${missingSigning.joinToString()}. See apps/android/README.md."
        }
        check(releaseKeystore?.isFile == true) { "The release keystore file does not exist." }
    }
}

val buildCore by tasks.registering(Exec::class) {
    description = "Builds crates/mobile for Android and generates Kotlin bindings."
    val ndk = System.getenv("ANDROID_NDK_HOME")
        ?: androidComponents.sdkComponents.sdkDirectory.get().dir("ndk/$NDK_VERSION").asFile.path
    val lib = coreOut.resolve("jniLibs/arm64-v8a/libzeron_mobile.so")
    val skip = skipCore
    workingDir = repoRoot
    commandLine("bash", "scripts/android/build-core.sh", coreOut.path)
    environment("ANDROID_NDK_HOME", ndk)
    onlyIf { !skip || !lib.exists() }
}

// Tool and file icons: the iOS asset catalog's SVGs, rasterized.
val genIcons by tasks.registering(Exec::class) {
    description = "Rasterizes the shared transcript icons."
    val out = iconsOut.get().asFile
    inputs.dir(repoRoot.resolve("apps/ios/Zeron/Assets.xcassets"))
    inputs.dir(repoRoot.resolve("crates/ui/assets/icons"))
    inputs.file(repoRoot.resolve("scripts/android/svg2vd.py"))
    outputs.dir(out)
    commandLine("bash", repoRoot.resolve("scripts/android/gen-icons.sh").path, out.resolve("assets/icons").path, out.resolve("res").path)
}

android {
    namespace = "sh.zeron.android"
    compileSdk = 37
    buildToolsVersion = "36.0.0"
    ndkVersion = NDK_VERSION

    defaultConfig {
        applicationId = "work.puqing.zerun.android"
        minSdk = 29
        targetSdk = 37
        versionCode = forkVersionCode.toInt()
        versionName = forkVersion
        buildConfigField(
            "String", "RELEASE_CERTIFICATE_SHA256",
            "\"${repoRoot.resolve("apps/android/release-certificate.sha256").readText().trim()}\"",
        )
    }

    signingConfigs {
        if (missingSigning.isEmpty()) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = releaseSigning["ZERUN_ANDROID_STORE_PASSWORD"]
                keyAlias = releaseSigning["ZERUN_ANDROID_KEY_ALIAS"]
                keyPassword = releaseSigning["ZERUN_ANDROID_KEY_PASSWORD"]
            }
        }
    }

    buildTypes {
        debug {
            ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            ndk { abiFilters += "arm64-v8a" }
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].apply {
        kotlin.directories.add(coreOut.resolve("kotlin").path)
        jniLibs.directories.add(coreOut.resolve("jniLibs").path)
        // The exact font bytes the Rust layout engine measures.
        assets.directories.add(repoRoot.resolve("apps/ios/Zeron/Fonts").path)
        assets.directories.add(iconsOut.get().asFile.resolve("assets").path)
        res.directories.add(iconsOut.get().asFile.resolve("res").path)
    }

    packaging { jniLibs { useLegacyPackaging = false } }
}

tasks.named("preBuild") { dependsOn(buildCore, genIcons) }
tasks.matching { it.name == "preReleaseBuild" }.configureEach {
    dependsOn(validateReleaseSigning)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.browser)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    debugImplementation(libs.compose.ui.tooling)
    testImplementation("junit:junit:4.13.2")
}

kotlin {
    compilerOptions {
        optIn.addAll(
            "androidx.compose.material3.ExperimentalMaterial3Api",
            "androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
        )
    }
}
