#!/bin/bash
# Build plumcraft-ffi with its JNI shims as the Android library:
# native/android/jniLibs/<abi>/libplumcraft_ffi.so (not committed), as the native virtimp.com
# app's LocalRenderer/build-android.sh does.
#
#   native/android/build-android.sh                     # arm64-v8a (phones, the Apple-silicon emulator)
#   ABIS="arm64-v8a x86_64" native/android/build-android.sh
#
# The NDK: $ANDROID_NDK_HOME, else the newest under $ANDROID_HOME/ndk (default: Homebrew's
# android-commandlinetools). Release without debug info, in the workspace's target/.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
jni="$here/jniLibs"
abis="${ABIS:-arm64-v8a}"
api="${ANDROID_API:-29}"   # research note 24, decision 12: Android 10
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/homebrew/share/android-commandlinetools}}"
ndk="${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)}"
if [ -z "$ndk" ] || [ ! -d "$ndk" ]; then
  echo "no Android NDK: set ANDROID_NDK_HOME, or install one under $sdk/ndk" >&2
  exit 1
fi
bin="$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=debuginfo

for abi in $abis; do
  case "$abi" in
    arm64-v8a) t=aarch64-linux-android ;;
    x86_64) t=x86_64-linux-android ;;
    *) echo "unknown ABI $abi" >&2; exit 2 ;;
  esac
  T=$(echo "$t" | tr a-z- A-Z_)
  export "CARGO_TARGET_${T}_LINKER=$bin/$t$api-clang"
  export "CC_${t//-/_}=$bin/$t$api-clang" "CXX_${t//-/_}=$bin/$t$api-clang++" "AR_${t//-/_}=$bin/llvm-ar"
  (cd "$root" && cargo rustc --release -p plumcraft-ffi --lib --crate-type cdylib --features jni --target "$t")
  mkdir -p "$jni/$abi"
  cp "$root/target/$t/release/libplumcraft_ffi.so" "$jni/$abi/"
  echo "built $jni/$abi/libplumcraft_ffi.so ($(du -h "$jni/$abi/libplumcraft_ffi.so" | cut -f1), NDK $(basename "$ndk"), API $api)"
done
