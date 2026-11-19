#!/bin/bash
# The Kotlin kit's tests on the host JVM: build crates/ffi for this Mac with the `jni` feature
# (release, no debug info, in the workspace's target/), then run the Gradle tests against it.
#
#   native/android/run-jvm-test.sh                 # all tests
#   native/android/run-jvm-test.sh --tests '*Latency*'
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
host="${HOST_TARGET:-aarch64-apple-darwin}"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=debuginfo
# as build-xcframework.sh sets it, so the two builds share one set of objects in target/
export MACOSX_DEPLOYMENT_TARGET=15.0
(cd "$root" && cargo build --release -p plumcraft-ffi --lib --features jni --target "$host")
export PLUMCRAFT_FFI_LIB_DIR="$root/target/$host/release"
cd "$here" && ./gradlew :plumcraft:test "$@"
