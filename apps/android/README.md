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

The APK is at `app/build/outputs/apk/debug/app-debug.apk`. This is a debug
build; production signing, app updates and Android CI are still pending.

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
