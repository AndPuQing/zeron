# Zerun for Android

A Jetpack Compose viewport onto the zeron mesh, built on the same Rust mobile
core as the iOS app (`crates/mobile`). **Rust decides what to paint and
where; Kotlin paints, scrolls and handles gestures.** The transcript's text
measurement, markdown layout, prefix-sum virtualization and display lists are
the exact code iOS runs — see [`docs/mobile-rewrite.md`](../../docs/mobile-rewrite.md).

The UI is Material 3 Expressive (`MaterialExpressiveTheme`, expressive motion,
flexible app bars, shape-morphing loading indicators, connected button groups,
segmented lists) themed with Zeron's own palette and Geist type.

## Build & run

Requires JDK 21, the Android SDK (platform 37.0, Build Tools 36.0.0,
NDK 29.0.14206865), and Rust
with `cargo install cargo-ndk` and
`rustup target add aarch64-linux-android x86_64-linux-android`.

```sh
cd apps/android
./gradlew :app:installDebug
```

The wrapper downloads Gradle from Huawei Cloud and repositories prefer Aliyun
mirrors, retaining the original Maven
repositories as fallbacks. The Gradle distribution's SHA256 is verified.
GitHub Actions uses the official Gradle distribution and sets
`ZERUN_CHINA_MIRRORS=false` to use Google Maven, Maven Central and the Gradle
Plugin Portal directly. CI also installs the SDK/NDK and Rust from official
sources.

The application ID is `work.puqing.zerun.android`, the display name is `Zerun`,
and WorkOS returns to `zerun-dev://callback`. Endpoints and WorkOS settings
come from the shared Rust core. Kotlin packages retain `sh.zeron.android`.
The pending OAuth state survives process recreation and is consumed once.

The app version follows the workspace's numeric version in `Cargo.toml`.
Android `versionCode` is `major * 1,000,000 + minor * 1,000 + patch`; minor
and patch components must stay below 1,000. For example, `0.3.0` is `3000`
and `0.3.1` is `3001`.

Build and run the login-state regression tests with:

```sh
./gradlew :app:testDebugUnitTest :app:assembleDebug
```

The APK is at `app/build/outputs/apk/debug/app-debug.apk`.

## Release signing

Release builds require a dedicated production key. They fail when credentials
are missing and never fall back to the Debug signing key. Keep the same key
for every release: Android requires it to upgrade an installed app. A Debug
installation must be uninstalled before installing a production APK with the
same application ID; export any local data first.

Provide these environment variables without putting passwords in command
arguments or tracked files:

| Variable | Value |
| --- | --- |
| `ZERUN_ANDROID_KEYSTORE` | Absolute path to the release keystore |
| `ZERUN_ANDROID_STORE_PASSWORD` | Keystore password |
| `ZERUN_ANDROID_KEY_ALIAS` | Signing key alias |
| `ZERUN_ANDROID_KEY_PASSWORD` | Signing key password |

Back up your release keystore securely; Actions Secrets cannot recover the
original private key. With the required environment variables set, build from
the repository root:

```sh
bash scripts/package-android.sh
```

The public certificate SHA256 in `release-certificate.sha256` pins the signing
identity. The packaging script rejects a different keystore certificate, so
replacing Actions Secrets cannot silently produce an incompatible update.

The script runs unit tests, builds Release with the configuration cache disabled
so signing credentials are not serialized into it, verifies the certificate
against the pinned identity, checks 16 KiB ZIP alignment and requires the native core
for both arm64-v8a and x86_64. The outputs are
`target/package/zerun-<version>-android.apk` and its `.apk.sha256` checksum.
Direct Gradle release builds should also pass `--no-configuration-cache`.

## CI and publication

The `Android` workflow tests and builds a Debug APK for Android-related pushes
and pull requests targeting `dev`. Download the `android-debug` workflow
artifact to install it. These builds do not need production signing secrets.

Signed builds use these repository Actions Secrets:

| Secret | Value |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | Base64-encoded release keystore |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `ANDROID_KEY_ALIAS` | Signing key alias |
| `ANDROID_KEY_PASSWORD` | Signing key password |

Run the `Android` workflow manually on `dev` with `release=true` to test the
signed pipeline without publishing a release. Its `android-release` artifact
contains the APK and checksum. The temporary keystore is removed after the build.

The existing `release` workflow calls the same signed build and requires it
to succeed before publication. On a `v<version>` tag matching `Cargo.toml`,
it publishes the APK alongside the desktop artifacts to GitHub Release and
the configured R2 download source at
`https://zerun.puqing.work/releases/zerun-<version>-android.apk`. The shared
`manifest.json` includes the APK's SHA256; `latest.txt` is updated only after
artifacts and the manifest are uploaded. A manual `release` run builds
artifacts without publishing them.

## In-app updates

The app checks the production `releases/manifest.json` source when returning to
the foreground, at most once an hour after a successful check. Settings →
**Check for updates** performs a check immediately. The sign-in screen has the
same entry, so updating does not require an account. A newer Android version
opens a dismissible update prompt; downloads start only when requested.

The app streams the universal APK over HTTPS into its private cache and shows
download progress. It rejects missing checksums, incomplete downloads, a SHA256
mismatch, a different application ID, a version different from the manifest,
downgrades, and signing certificates different from the pinned production
identity. Only a verified APK is made available to Android's installer through
a narrowly scoped FileProvider. Failed downloads can be retried.

**Install** opens Android's per-app installation permission screen if needed.
Allow Zerun to install apps, then return to the app; the system installer asks
for confirmation. Installation is never silent. Cancelling installation leaves
the verified download available for another attempt. A completed download is
revalidated after restarting the app, and obsolete downloads are cleaned up
after upgrading. Android's package replacement preserves account data and
settings; the app does not clear them during updates.

Version 0.3.0 predates this updater: install a newer production APK once to
enable future in-app updates. Debug APKs have a different signer and cannot be
upgraded to production APKs in place. Download-cache eviction or terminating
the app during an incomplete download requires downloading again.

## Shared core and assets

The `buildCore` task runs `scripts/android/build-core.sh`, which builds
`crates/mobile` for Android (`jniLibs`) and generates its Kotlin bindings into
`target/android-core/`. `-PzeronSkipCore` reuses the last build while
iterating on Kotlin; `ZERON_ANDROID_ABIS=arm64-v8a` builds one ABI.
`scripts/android/gen-icons.sh` rasterizes the shared tool/file SVG icons
(needs `rsvg-convert`). The Geist fonts are read straight from the iOS app's
`Fonts/` folder, so both platforms measure and draw the same bytes.

## Layout

```
core/        AppModel (owns CoreClient, republishes snapshots as flows),
             CredentialStore, Fonts + AndroidMeasurer (Minikin fallback
             measurement for glyphs Geist lacks)
design/      ZeronTheme (Material 3 Expressive), transcript palette
transcript/  TranscriptState (layout engine + viewport: anchoring, follow the
             tail), Transcript (virtualized rows over LayoutFrame), RowModel
             (canvas painter for Rust display lists, streaming veil, fades),
             Widgets (copy, disclosures, tool rail, shimmer, images…)
ui/          Sign-in, sessions, session + composer, new session, search,
             settings
```

## Launch extras

Mirrors the iOS launch arguments:

```sh
adb shell am start -n work.puqing.zerun.android/sh.zeron.android.MainActivity \
  --ez demo true --es route chat:chat-veil
```

| Extra | Effect |
| --- | --- |
| `--ez demo true` | Offline demo workspace (Rust `DemoHost`) |
| `--ez fast true` / `--ez longreply true` | Demo stream speed / reply length |
| `--ez big true` / `--ez huge true` | Demo transcripts with 120 / 600 turns |
| `--es route chat:<id>` / `new` / `search` / `settings` | Open a screen at launch |
| `--ez signedout true` | Clear stored credentials |
| `--es wallpaper <path>` / `none` | Set (or clear) the wallpaper from a file the app can read, e.g. `adb push art.jpg /data/local/tmp/ && adb shell run-as work.puqing.zerun.android cp /data/local/tmp/art.jpg files/` then `--es wallpaper /data/user/0/work.puqing.zerun.android/files/art.jpg` |
| `--es wallpaper-effect <none\|dither\|ascii\|halftone\|scanlines>` | Wallpaper effect |
