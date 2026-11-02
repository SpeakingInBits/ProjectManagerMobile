#!/usr/bin/env bash
# Builds the iOS app into dist/ProjectManager-ios.ipa.
#
#   VERSION=1.2.3 package-ios.sh
#
# Runs on a Mac with Xcode, XcodeGen and the Rust target aarch64-apple-ios (see "Building for iOS"
# in the README).
#
# The app is signed when IOS_CERTIFICATE_P12_BASE64 (a base64-encoded .p12 holding the signing
# certificate and its private key), IOS_CERTIFICATE_PASSWORD and IOS_PROVISIONING_PROFILE_BASE64
# (a base64-encoded .mobileprovision for com.speakinginbits.projectmanager) are set. The profile's
# type decides the export: development, ad hoc, enterprise or App Store. Otherwise the app is
# unsigned and named ProjectManager-ios-unsigned.ipa.
set -euo pipefail

version=${VERSION:?set VERSION, e.g. VERSION=1.2.3}
root=$(cd "$(dirname "$0")/.." && pwd)
dist="$root/dist"
build="$root/app/ios/build"
archive="$build/ProjectManager.xcarchive"
bundle_id=com.speakinginbits.projectmanager

cd "$root/app/ios"
xcodegen generate
rm -rf "$build"
mkdir -p "$build" "$dist"

archive_args=(
  -project ProjectManager.xcodeproj -scheme ProjectManager -configuration Release
  -destination generic/platform=iOS -archivePath "$archive"
  MARKETING_VERSION="$version" CURRENT_PROJECT_VERSION="$version"
)

if [ -z "${IOS_CERTIFICATE_P12_BASE64:-}" ] || [ -z "${IOS_PROVISIONING_PROFILE_BASE64:-}" ]; then
  echo "::warning::IOS_CERTIFICATE_P12_BASE64 or IOS_PROVISIONING_PROFILE_BASE64 isn't set, so the iOS app is unsigned."
  xcodebuild archive "${archive_args[@]}" CODE_SIGNING_ALLOWED=NO
  # An .ipa is the .app inside a Payload folder, zipped.
  mkdir "$build/Payload"
  cp -R "$archive/Products/Applications/ProjectManager.app" "$build/Payload/"
  (cd "$build" && zip -qry "$dist/ProjectManager-ios-unsigned.ipa" Payload)
  ls -l "$dist"
  exit 0
fi

: "${IOS_CERTIFICATE_PASSWORD:?IOS_CERTIFICATE_PASSWORD is needed with IOS_CERTIFICATE_P12_BASE64}"

# The certificate goes in a keychain of its own, and it and the profile are removed afterwards.
keychain="$build/signing.keychain-db"
keychain_password=$(openssl rand -hex 16)
profile="$build/profile.mobileprovision"
profile_dirs=(
  "$HOME/Library/MobileDevice/Provisioning Profiles"
  "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
)
uuid=
cleanup() {
  security delete-keychain "$keychain" 2> /dev/null || true
  if [ -n "$uuid" ]; then
    for dir in "${profile_dirs[@]}"; do rm -f "$dir/$uuid.mobileprovision"; done
  fi
}
trap cleanup EXIT

security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
printf '%s' "$IOS_CERTIFICATE_P12_BASE64" | base64 --decode > "$build/certificate.p12"
security import "$build/certificate.p12" -k "$keychain" -P "$IOS_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign -T /usr/bin/security
rm "$build/certificate.p12"
# Lets codesign use the key without a password prompt.
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" > /dev/null
# codesign only looks in the keychain search list.
read -ra keychains <<< "$(security list-keychains -d user | tr -d '"' | xargs)"
security list-keychains -d user -s "$keychain" "${keychains[@]}"

# find-identity lists e.g.  1) 0123ABCD... "Apple Distribution: Speaking In Bits (TEAMID1234)"
identity=$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/\)/ { print $2; exit }')
if [ -z "$identity" ]; then
  echo "::error::IOS_CERTIFICATE_P12_BASE64 has no valid code signing certificate."
  exit 1
fi

printf '%s' "$IOS_PROVISIONING_PROFILE_BASE64" | base64 --decode > "$profile"
security cms -D -i "$profile" > "$build/profile.plist"
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$build/profile.plist" 2> /dev/null; }
uuid=$(plist UUID)
profile_name=$(plist Name)
team=$(plist TeamIdentifier:0)
for dir in "${profile_dirs[@]}"; do
  mkdir -p "$dir"
  cp "$profile" "$dir/$uuid.mobileprovision"
done

if plist ProvisionsAllDevices > /dev/null; then
  method=enterprise
elif plist ProvisionedDevices > /dev/null; then
  if [ "$(plist Entitlements:get-task-allow || true)" = true ]; then
    method=debugging
  else
    method=release-testing
  fi
else
  method=app-store-connect
fi
echo "Signing with \"$identity\" and profile \"$profile_name\" ($method)."

xcodebuild archive "${archive_args[@]}" \
  CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM="$team" CODE_SIGN_IDENTITY="$identity" \
  PROVISIONING_PROFILE_SPECIFIER="$profile_name" OTHER_CODE_SIGN_FLAGS="--keychain $keychain"

cat > "$build/ExportOptions.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key>
  <string>$method</string>
  <key>teamID</key>
  <string>$team</string>
  <key>signingStyle</key>
  <string>manual</string>
  <key>signingCertificate</key>
  <string>$identity</string>
  <key>provisioningProfiles</key>
  <dict>
    <key>$bundle_id</key>
    <string>$profile_name</string>
  </dict>
</dict>
</plist>
EOF
xcodebuild -exportArchive -archivePath "$archive" -exportPath "$build/export" \
  -exportOptionsPlist "$build/ExportOptions.plist"
cp "$build"/export/*.ipa "$dist/ProjectManager-ios.ipa"
ls -l "$dist"
