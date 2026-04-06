import java.io.ByteArrayOutputStream

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
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
        export ANDROID_NDK_HOME=/Users/xcl/Library/Android/sdk/ndk-bundle
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

android {
    namespace = "com.inkprint.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.inkprint.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 11
        versionName = "0.11"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables {
            useSupportLibrary = true
        }

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    signingConfigs {
        create("release") {
            storeFile = file("${rootProject.rootDir}/inkprint-release.jks")
            storePassword = "inkprint123"
            keyAlias = "inkprint"
            keyPassword = "inkprint123"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        compose = true
    }
    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.8"
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
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.7.0")
    implementation("androidx.activity:activity-compose:1.8.2")
    implementation(platform("androidx.compose:compose-bom:2024.02.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("net.java.dev.jna:jna:5.13.0@aar")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2024.02.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
