.PHONY: all rust-test rust-build-android uniffi-bindings android-debug android-release clean

NDK_HOME ?= /opt/homebrew/share/android-ndk
RUSTC ?= $(HOME)/.rustup/toolchains/nightly-aarch64-apple-darwin/bin/rustc
JAVA_HOME ?= /opt/homebrew/opt/openjdk@17
ANDROID_HOME ?= $(HOME)/Library/Android/sdk
ANDROID_DIR = android

all: rust-test

# Run Rust unit tests on host
rust-test:
	cargo test -p inkprint-core

# Build Rust .so for Android (arm64-v8a)
rust-build-android:
	RUSTC=$(RUSTC) ANDROID_NDK_HOME=$(NDK_HOME) ~/.cargo/bin/cargo ndk -t arm64-v8a -o $(shell pwd)/$(ANDROID_DIR)/app/src/main/jniLibs build --release -p inkprint-core

# Generate UniFFI Kotlin bindings
uniffi-bindings:
	cargo run -p uniffi_bindgen -- generate \
		inkprint-core/src/inkprint.udl \
		--language kotlin \
		--out-dir $(ANDROID_DIR)/app/src/main/kotlin/com/inkprint/uniffi

# Build debug APK (also runs cargo-ndk)
android-debug: rust-build-android uniffi-bindings
	JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew assembleDebug

# Build release APK
android-release: rust-build-android uniffi-bindings
	JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew assembleRelease

clean:
	cargo clean
	cd $(ANDROID_DIR) && ./gradlew clean
