.PHONY: all rust-test epub-test rust-build-android rust-build-android-fdroid uniffi-bindings \
	fetch-native-deps fetch-host-deps android-debug android-release android-bundle android-fdroid clean

NDK_HOME ?= /opt/homebrew/share/android-ndk
RUSTC ?= $(HOME)/.rustup/toolchains/nightly-aarch64-apple-darwin/bin/rustc
JAVA_HOME = /opt/homebrew/opt/openjdk@17
ANDROID_HOME ?= $(HOME)/Library/Android/sdk
ANDROID_DIR = android
JNI_DIR = $(shell pwd)/$(ANDROID_DIR)/app/src/main/jniLibs

# Prebuilt native libraries for the EPUB printer, pinned by checksum.
# libonnxruntime.so itself comes from the onnxruntime-android AAR (Gradle).
PDFIUM_URL = https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8086
PDFIUM_ANDROID_SHA256 = f6ea29495d64795b9af61e41768030795b45f82a21415498ee4ab2c61e135756
PDFIUM_MAC_SHA256 = e98679e052c07edbb5a627980902abb823d4b3f35744d877bd21668bd9fc13ab
ORT_VERSION = 1.30.0
ORT_MAC_SHA256 = 6ebb5062a934537c352937821f9fe9718e7de1a2db1122a93dd363ffd53a7012
NATIVE = $(shell pwd)/native
PDFIUM_ANDROID = $(ANDROID_DIR)/app/src/full/jniLibs/arm64-v8a/libpdfium.so
PDFIUM_MAC = $(NATIVE)/pdfium-mac/lib/libpdfium.dylib
ORT_MAC = $(NATIVE)/onnxruntime-osx-arm64-$(ORT_VERSION)/lib/libonnxruntime.dylib
MODELS = $(shell pwd)/$(ANDROID_DIR)/app/src/full/assets/models

all: rust-test

# Run Rust unit tests on host (the EPUB end-to-end tests skip themselves)
rust-test:
	cargo test -p inkprint-core -p inkprint-epub

# EPUB end-to-end tests with real pdfium, ONNX Runtime and models (macOS arm64)
epub-test: fetch-host-deps
	INKPRINT_PDFIUM=$(PDFIUM_MAC) INKPRINT_ORT=$(ORT_MAC) INKPRINT_MODELS=$(MODELS) \
		cargo test -p inkprint-epub

# Build Rust .so for Android (arm64-v8a)
rust-build-android:
	RUSTC=$(RUSTC) ANDROID_NDK_HOME=$(NDK_HOME) ~/.cargo/bin/cargo ndk -t arm64-v8a -o $(JNI_DIR) build --release -p inkprint-core
	@# cargo-ndk also copies pdfium-render's own cdylib, which nothing loads.
	rm -f $(JNI_DIR)/arm64-v8a/libpdfium_render-*.so

# Rust .so for the F-Droid flavor: no EPUB printer, no prebuilt native deps
rust-build-android-fdroid:
	RUSTC=$(RUSTC) ANDROID_NDK_HOME=$(NDK_HOME) ~/.cargo/bin/cargo ndk -t arm64-v8a -o $(JNI_DIR) build --release -p inkprint-core --no-default-features

# Generate UniFFI Kotlin bindings
uniffi-bindings:
	cargo run -p uniffi-bindgen -- generate \
		inkprint-core/src/inkprint.udl \
		--language kotlin \
		--out-dir $(ANDROID_DIR)/app/src/main/kotlin/com/inkprint/uniffi

fetch-native-deps: $(PDFIUM_ANDROID)

fetch-host-deps: $(PDFIUM_MAC) $(ORT_MAC)

$(PDFIUM_ANDROID):
	mkdir -p $(NATIVE)/dl $(NATIVE)/pdfium-android $(dir $@)
	curl -fsSL --retry 5 --retry-all-errors -o $(NATIVE)/dl/pdfium-android-arm64.tgz $(PDFIUM_URL)/pdfium-android-arm64.tgz
	echo "$(PDFIUM_ANDROID_SHA256)  $(NATIVE)/dl/pdfium-android-arm64.tgz" | shasum -a 256 -c -
	tar -xzf $(NATIVE)/dl/pdfium-android-arm64.tgz -C $(NATIVE)/pdfium-android
	cp $(NATIVE)/pdfium-android/lib/libpdfium.so $@

$(PDFIUM_MAC):
	mkdir -p $(NATIVE)/dl $(NATIVE)/pdfium-mac
	curl -fsSL --retry 5 --retry-all-errors -o $(NATIVE)/dl/pdfium-mac-arm64.tgz $(PDFIUM_URL)/pdfium-mac-arm64.tgz
	echo "$(PDFIUM_MAC_SHA256)  $(NATIVE)/dl/pdfium-mac-arm64.tgz" | shasum -a 256 -c -
	tar -xzf $(NATIVE)/dl/pdfium-mac-arm64.tgz -C $(NATIVE)/pdfium-mac

$(ORT_MAC):
	mkdir -p $(NATIVE)/dl
	curl -fsSL --retry 5 --retry-all-errors -o $(NATIVE)/dl/onnxruntime-osx.tgz https://github.com/microsoft/onnxruntime/releases/download/v$(ORT_VERSION)/onnxruntime-osx-arm64-$(ORT_VERSION).tgz
	echo "$(ORT_MAC_SHA256)  $(NATIVE)/dl/onnxruntime-osx.tgz" | shasum -a 256 -c -
	tar -xzf $(NATIVE)/dl/onnxruntime-osx.tgz -C $(NATIVE)

# Build debug APK (also runs cargo-ndk)
android-debug: rust-build-android uniffi-bindings fetch-native-deps
	cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew assembleFullDebug

# Build release APK
android-release: rust-build-android uniffi-bindings fetch-native-deps
	cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew assembleFullRelease

# Build release AAB — this is what Google Play accepts as an upload
android-bundle: rust-build-android uniffi-bindings fetch-native-deps
	cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew bundleFullRelease

# Release APK of the F-Droid flavor (what F-Droid's build recipe produces)
android-fdroid: rust-build-android-fdroid uniffi-bindings
	cd $(ANDROID_DIR) && JAVA_HOME=$(JAVA_HOME) ANDROID_HOME=$(ANDROID_HOME) ./gradlew assembleFdroidRelease

clean:
	cargo clean
	cd $(ANDROID_DIR) && ./gradlew clean
