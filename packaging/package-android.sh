#!/usr/bin/env bash
# Builds the Android APK and Play Store app bundle into dist/.
#
#   VERSION=1.2.3 package-android.sh
#
# Needs ANDROID_HOME, ANDROID_NDK_HOME, a JDK (17 or later), cargo-ndk and the Rust targets
# aarch64-linux-android and x86_64-linux-android (see "Building for Android" in the README).
#
# The packages are signed when ANDROID_KEYSTORE_BASE64 (a base64-encoded keystore),
# ANDROID_KEYSTORE_PASSWORD and ANDROID_KEY_ALIAS are set (ANDROID_KEY_PASSWORD too, if the key's
# password differs from the keystore's). Otherwise they're unsigned and named *-unsigned.
set -euo pipefail

version=${VERSION:?set VERSION, e.g. VERSION=1.2.3}
root=$(cd "$(dirname "$0")/.." && pwd)
dist="$root/dist"

# Play requires a bigger versionCode for every upload; 1.2.3 becomes 1002003.
IFS=. read -r major minor patch <<< "$version"
version_code=$((10#$major * 1000000 + 10#$minor * 1000 + 10#$patch))

suffix=-unsigned
if [ -n "${ANDROID_KEYSTORE_BASE64:-}" ]; then
  : "${ANDROID_KEYSTORE_PASSWORD:?ANDROID_KEYSTORE_PASSWORD is needed with ANDROID_KEYSTORE_BASE64}"
  : "${ANDROID_KEY_ALIAS:?ANDROID_KEY_ALIAS is needed with ANDROID_KEYSTORE_BASE64}"
  keystore="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/release.jks"
  printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 --decode > "$keystore"
  trap 'rm -f "$keystore"' EXIT
  export ANDROID_KEYSTORE_PATH="$keystore"
  # An empty secret still sets the variable; Gradle falls back to the keystore password when it's unset.
  if [ -z "${ANDROID_KEY_PASSWORD:-}" ]; then
    unset ANDROID_KEY_PASSWORD
  fi
  suffix=
else
  echo "::warning::ANDROID_KEYSTORE_BASE64 isn't set, so the Android packages are unsigned."
fi

cd "$root/app/android"
./gradlew --no-daemon assembleRelease bundleRelease -PversionName="$version" -PversionCode="$version_code"

outputs="$root/app/android/app/build/outputs"
mkdir -p "$dist"
# Gradle names the APK app-release.apk when it signs it and app-release-unsigned.apk when it doesn't.
cp "$outputs"/apk/release/app-release*.apk "$dist/ProjectManager-android$suffix.apk"
cp "$outputs/bundle/release/app-release.aab" "$dist/ProjectManager-android$suffix.aab"
ls -l "$dist"
