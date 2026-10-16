#!/bin/bash
# Rebuild JNA's libjnidispatch.so for arm64-v8a with the local NDK.
#
# The copy inside the JNA AAR is built with NDK r10e (GCC 4.9). Its segments
# are 16 KB aligned, but Google Play still flags it as built with an old NDK,
# so the Play ("full") flavor ships this rebuild instead. Same JNA tag, same
# native version string, so the JNA classes accept it unchanged.
#
# Usage: scripts/build-jnidispatch.sh <work dir> <output .so>
# Needs git, a JDK (JAVA_HOME), autoconf/automake/libtool and the NDK.
set -euo pipefail

WORK=${1:?work dir}
OUT=${2:?output .so path}
NDK=${ANDROID_NDK_HOME:-/opt/homebrew/share/android-ndk}
JAVA_HOME=${JAVA_HOME:-/opt/homebrew/opt/openjdk@17}

# Keep these in step with the JNA version in android/app/build.gradle.kts.
JNA_TAG=5.19.1
JNA_COMMIT=1a91122853f6ab6f1fb2a4a284a6cf2ed8af0a4d
# Version.VERSION_NATIVE and API checksum of JNA 5.19.1's own build.
JNI_VERSION=7.0.4
JNI_CHECKSUM=5fb98531302accd485c534c452dd952a

API=26
TC=$(ls -d "$NDK"/toolchains/llvm/prebuilt/* | head -1)/bin
CC="$TC/aarch64-linux-android$API-clang"
SRC=$WORK/jna

if [ ! -d "$SRC" ]; then
  git clone -q --depth 1 --branch "$JNA_TAG" --filter=blob:none --sparse \
    https://github.com/java-native-access/jna.git "$SRC"
  git -C "$SRC" sparse-checkout set native src/com/sun/jna
fi
[ "$(git -C "$SRC" rev-parse HEAD)" = "$JNA_COMMIT" ] || { echo "JNA $JNA_TAG is not $JNA_COMMIT" >&2; exit 1; }

# JNI headers for dispatch.c / callback.c
mkdir -p "$WORK/headers" "$WORK/classes"
"$JAVA_HOME/bin/javac" -nowarn -d "$WORK/classes" -h "$WORK/headers" \
  $(find "$SRC/src/com/sun/jna" -name '*.java') 2>/dev/null

# libffi, statically linked as in JNA's own Android build
FFI=$SRC/native/libffi
if [ ! -x "$FFI/configure" ]; then
  (cd "$FFI" && LIBTOOLIZE=$(command -v libtoolize || command -v glibtoolize) autoreconf -fi >/dev/null 2>&1)
fi
DEFS="-DFFI_STATIC_BUILD -DNO_JAWT -DNO_WEAK_GLOBALS -DFFI_MMAP_EXEC_WRIT=1 -DFFI_MMAP_EXEC_SELINUX=0"
COPT="-O2 -fno-omit-frame-pointer -fno-strict-aliasing -fpic -ffunction-sections -funwind-tables -fno-short-enums"
COPT+=" -Wno-implicit-function-declaration -Wno-int-conversion -Wno-incompatible-function-pointer-types"
B=$WORK/build
rm -rf "$B" && mkdir -p "$B/libffi"
(cd "$B/libffi" && CC="$CC" AR="$TC/llvm-ar" RANLIB="$TC/llvm-ranlib" CFLAGS="$COPT" CPPFLAGS="$DEFS" \
  "$FFI/configure" --host=aarch64-linux-android --enable-static --disable-shared --with-pic=yes \
    --disable-docs --disable-multi-os-directory >configure.log 2>&1 && make -j8 >make.log 2>&1) \
  || { echo "libffi build failed, see $B/libffi/*.log" >&2; exit 1; }

for f in dispatch callback; do
  "$CC" $COPT $DEFS -D_REENTRANT -I"$WORK/headers" -I"$B/libffi/include" \
    -DJNA_JNI_VERSION="\"$JNI_VERSION\"" -DCHECKSUM="\"$JNI_CHECKSUM\"" \
    -c "$SRC/native/$f.c" -o "$B/$f.o" 2>/dev/null
done
mkdir -p "$(dirname "$OUT")"
"$CC" -shared -o "$OUT" "$B/dispatch.o" "$B/callback.o" "$B/libffi/.libs/libffi.a" \
  -Wl,-Bsymbolic -Wl,--build-id=sha1 -Wl,-z,max-page-size=16384 -ldl -lm
"$TC/llvm-strip" --strip-unneeded "$OUT"
