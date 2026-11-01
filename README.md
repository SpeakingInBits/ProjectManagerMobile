# Project Manager (mobile)

A native Rust port of the [ProjectManager web app](https://github.com/SpeakingInBits/ProjectManager)
for iOS and Android (it also runs on Windows, macOS and Linux for development).

Same features as the web app: projects with due dates and progress bars, tasks that stand alone or
belong to a project, pinned tasks, repeating tasks (daily, weekly, monthly, yearly, every N days,
or N days after completion, with completed occurrences kept as history), a shared
category/subcategory taxonomy, and time tracking in 15-minute steps with day notes, history,
per-category totals, backfilling past days, and merging items. Everything stays on the device;
there are no accounts and no network access.

## Why it's light and fast

- **No web view, no JavaScript.** The UI is [Slint](https://slint.dev), compiled to native code and
  drawn on the GPU (OpenGL ES on Android, Skia/Metal on iOS).
- **One small data file.** Data is a single JSON document in the app's private storage, loaded into
  memory at start-up and written atomically (temp file + rename) after each change.

## Project layout

| Path | What it is |
| --- | --- |
| `core/` | `project-core`: data model, persistence, repeat scheduling, task ordering and time-tracking rollups. No UI dependencies; all the business logic and its tests live here. |
| `app/` | `project-app`: the Slint UI (`app/ui/*.slint`) and the Rust controller that connects it to `core` (`app/src`). |
| `app/android/` | Gradle project that packages the Rust library into an APK or Play Store app bundle. |
| `app/ios/` | XcodeGen spec, app icon and the build script Xcode runs to compile the app with cargo. |

How the web app maps to this one:

| Web (TypeScript) | Here (Rust) |
| --- | --- |
| `src/models/types.ts` | `core/src/model.rs` (same field names, so records serialize the same way) |
| `src/db/*` (IndexedDB) | `core/src/store.rs` (JSON file) |
| `src/domain/repeat.ts`, `progress.ts`, `taskSort.ts` | `core/src/domain.rs` and `Store::complete_task` |
| `src/domain/timeTracking.ts` | `core/src/time.rs` |
| `src/utils/dates.ts` | `core/src/date.rs` |
| `src/dev/seed.ts` | `core/src/demo.rs` |
| `src/pages/*`, `src/components/*` | `app/ui/*.slint`, with `app/src/controller.rs` and `viewmodel.rs` |

The domain rules carry over unchanged: completing a repeating task never resets it in place (the
completed row stays as history and a new row is created for the next occurrence), schedule-based
repeats count from the previous due date while "movable" counts from the completion date,
monthly and yearly repeats stay on their day of the month (a task due on the 31st repeats Jan 31,
Feb 28, Mar 31), a day
whose time drops to zero loses its note too, and deleting a category detaches every project, task
and time item that used it.

## Development

Requires Rust 1.92 or later.

```bash
cargo run -p project-app
```

Runs the app in a desktop window sized like a phone. Data goes to your user data folder
(`%APPDATA%\ProjectManager` on Windows); point it elsewhere for testing with:

```bash
cargo run -p project-app -- --data-dir ./scratch-data
```

`--seed` replaces that folder's data with a reproducible demo dataset (categories, projects, tasks
of every repeat kind, and time items with about ten weeks of history), like the web app's
`seedTestData()`. Never use it on a data folder holding real data:

```bash
cargo run -p project-app -- --data-dir ./scratch-data --seed
```

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets
```

The UI uses the Slint language; the [Slint VS Code extension](https://marketplace.visualstudio.com/items?itemName=Slint.slint)
gives live preview of `.slint` files.

## Building for Android

One-time setup:

1. Install [Android Studio](https://developer.android.com/studio). In **Settings → Languages &
   Frameworks → Android SDK**, install an SDK platform (API 36), and under **SDK Tools** the
   **NDK** and **Android SDK Command-line Tools**.
2. Set `ANDROID_HOME` to the SDK folder and `ANDROID_NDK_HOME` to the NDK folder inside it (for
   example `%LOCALAPPDATA%\Android\Sdk` and `%LOCALAPPDATA%\Android\Sdk\ndk\<version>`).
3. Install the Rust targets and cargo-ndk:

   ```bash
   rustup target add aarch64-linux-android x86_64-linux-android
   ```

   ```bash
   cargo install cargo-ndk
   ```

Then open `app/android` in Android Studio and press Run (the Gradle build calls cargo-ndk for you),
or from the command line in `app/android`, with `JAVA_HOME` pointing at JDK 17 or later (Android
Studio's bundled JDK works):

```bash
./gradlew installDebug
```

The build always compiles the Rust code in release mode, for both `arm64-v8a` (phones) and
`x86_64` (emulators).

Debug builds let you read or replace the app's data file over adb, which is handy for testing:

```bash
adb shell run-as com.speakinginbits.projectmanager cat files/project-manager.json
```

For the Play Store, build a signed app bundle with **Build → Generate Signed App Bundle** in Android
Studio, or `./gradlew bundleRelease` with a signing config. The bundle is
`app/android/app/build/outputs/bundle/release/app-release.aab`.

The app id is `com.speakinginbits.projectmanager`; change `applicationId` in
`app/android/app/build.gradle.kts` before the first upload if you want a different one. It can't
change after publishing.

## Building for iOS

Needs a Mac with Xcode and [XcodeGen](https://github.com/yonaskolb/XcodeGen).

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

```bash
cd app/ios && xcodegen
```

Open the generated `ProjectManager.xcodeproj`, choose your team under **Signing & Capabilities**,
and run on a simulator or device. Xcode runs `build_for_ios_with_cargo.bash`, which builds the
`project-manager` binary with cargo. For the App Store, use **Product → Archive**.

The bundle id is `com.speakinginbits.projectmanager` (set in `app/ios/project.yml`).

## Your data

Data is stored in a single file, `project-manager.json`, in the app's private storage: internal
storage on Android, `Library/Application Support` in the app container on iOS. Android cloud backup
is turned off so data never leaves the device. Uninstalling the app deletes the data.

The file uses the same record shapes and field names as the web app's IndexedDB stores
(`categories`, `subcategories`, `projects`, `tasks`, `timeEntries`), so data can be moved between
the two once an import/export feature exists. A file that can't be read is renamed to
`project-manager.corrupt.json` rather than overwritten.

## Differences from the web app

- **Navigation:** four bottom tabs (Projects, Tasks, Time, Categories). The page title, **Add**
  and settings buttons live in the top bar, a project opens as its own screen with a back arrow,
  and forms open as bottom sheets. The Android back button closes the open dialog, sheet or
  project.
- **Dark mode:** the top-bar button cycles between following the system, light and dark.
- **Settings are stored in the data file** (task completion confirmation, time-list grouping and
  sorting, theme) instead of the browser's localStorage. The time-tracking entry date still resets
  to today each time the app starts.
- **Deleting a project with tasks** asks once, with **Delete project and tasks**, **Keep tasks as
  standalone** and **Cancel**, instead of two OK/Cancel prompts.
- **Categories** are renamed in a sheet instead of inline, and are listed in the order they were
  created.
- **The − button** on a time item is disabled when nothing is logged on the entry day (the web app
  disables it only when the item has no time at all).
- **Demo data** comes from the desktop build's `--seed` option instead of the devtools console.
