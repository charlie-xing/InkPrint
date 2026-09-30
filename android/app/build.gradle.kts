plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Run cargo-ndk to build Rust library and generate UniFFI bindings
val cargoNdkBuild by tasks.registering(Exec::class) {
    group = "build"
    description = "Build Rust library via cargo-ndk and generate UniFFI Kotlin bindings"

    workingDir = rootProject.rootDir.parentFile  // inkprint workspace root
    commandLine(
        "bash", "-c",
        """
        set -e
        export ANDROID_NDK_HOME=${'$'}{ANDROID_NDK_HOME:-/opt/homebrew/share/android-ndk}
        cargo ndk -t arm64-v8a -o ${project.projectDir}/src/main/jniLibs build --release -p inkprint-core
        cargo run --bin uniffi-bindgen generate \
            inkprint-core/src/inkprint.udl \
            --language kotlin \
            --out-dir ${project.projectDir}/src/main/kotlin/com/inkprint/uniffi \
            2>/dev/null || \
        cargo run -p uniffi_bindgen -- generate \
            inkprint-core/src/inkprint.udl \
            --language kotlin \
            --out-dir ${project.projectDir}/src/main/kotlin/com/inkprint/uniffi \
            2>/dev/null || true
        """.trimIndent()
    )
}

// Release signing credentials never live in the repo. Supply them through
// ~/.gradle/gradle.properties or the environment — see android/gradle.properties
// for the property names. Without them, release builds are simply left unsigned.
fun secret(name: String): String? =
    (project.findProperty(name) as String?)?.takeIf { it.isNotBlank() }
        ?: System.getenv(name)?.takeIf { it.isNotBlank() }

val releaseStoreFile = secret("INKPRINT_STORE_FILE")?.let { file(it) }?.takeIf { it.exists() }

android {
    namespace = "com.inkprint.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.inkprint.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 14
        versionName = "0.3"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables {
            useSupportLibrary = true
        }

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    signingConfigs {
        if (releaseStoreFile != null) {
            create("release") {
                storeFile = releaseStoreFile
                storePassword = secret("INKPRINT_STORE_PASSWORD")
                keyAlias = secret("INKPRINT_KEY_ALIAS")
                keyPassword = secret("INKPRINT_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.9.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    // Pulled in by Compose; pinned for its 16 KB-page-aligned native library
    // (the version in the Compose BOM above isn't). Google Play requires 16 KB
    // page support for apps targeting Android 15+.
    implementation("androidx.graphics:graphics-path:1.1.0")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.04.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
