# Shared mobile Liquid Glass

The mobile shells share a visual language, with platform renderers: UIKit's
native `UIGlassEffect` on iOS 26 and Compose's GPU-backed Backdrop on Android.
Rust still owns transcript geometry and session behavior.

## Component research

The following libraries inform the rendering and component choices.

| Project | Useful patterns | Decision |
| --- | --- | --- |
| [AndroidLiquidGlass / Backdrop](https://github.com/Kyant0/AndroidLiquidGlass) | Shared graphics layer, blur followed by edge refraction, highlights, independent foreground controls | Use 1.0.6, compatible with the existing Kotlin/Compose stack. 2.0.1 moves to Kotlin 2.4.10 and Compose Multiplatform 1.12. |
| [Haze](https://github.com/chrisbanes/haze) | Explicit source/effect separation, reusable regular/clear styles, performance profiles | Haze 2 glass is experimental; useful architecture reference. |
| [Cloudy](https://github.com/skydoves/Cloudy) | Distinguishes self blur from backdrop blur, shared capture source and low-version fallback | Broader effect set and CPU self-blur path are unnecessary for this shell. |
| [Liquid Glass Compose](https://github.com/Mortd3kay/liquid-glass-android) | AGSL lens and gradient fallback | Experimental 0.1.0 with a ten-element limit; prefer Backdrop. |
| [FloatingTabBar](https://github.com/elyesmansour/compose-floating-tab-bar) | Capsule tabs, separate search control, accessory retained across tab changes | Apply the composition and accessory pattern to the existing navigation. Scroll minimization is a future enhancement. |
| [Theseus](https://github.com/valzevul/Theseus) | UIKit glass controls and accessibility fallbacks for older iOS | Current deployment target is iOS 26; retain the native renderer. |
| [liquid_glass_widgets](https://github.com/sdegenaar/liquid_glass_widgets) | Consistent glass controls, restrained lighting and interaction | Visual reference; the project has native shells, so Flutter is not introduced. |

## Shared rules

| Token / component | Both shells |
| --- | --- |
| Page | `#F3F3F5` light / `#060606` dark |
| Text | `#27272C` light / `#E8E8EA` dark |
| Accent | `#5B43E8` light / `#8B7CF6` dark |
| Typeface | Bundled Geist; Geist Mono for project monograms and code |
| Page title | 30pt / 30sp, semibold |
| Toolbar action | 44pt / 44dp, circular visual shape |
| Composer | 25pt / 25dp radius, capsule when resting and card when active |
| Question card | 26pt / 26dp radius |
| Inner control wash | Foreground at 7.5% opacity |
| Selection | Neutral tab pill; quiet violet filter and project accents |
| User bubble | `#FFFFFF` light / `#19191C` dark |

iOS stores geometry in `Glass.Metrics`, Android in `GlassMetrics`. Keep these
values together when evolving components. Android's optional wallpaper-derived
accent remains a user preference; default colors match iOS.

Glass is used for floating navigation, search, toolbar actions, composers,
queued-message panels and agent questions. Transcript text, code blocks and
lists retain stable readable fills. Inline controls inside a captured page use
the same material colors without sampling their own layer.

## Android rendering

`GlassScene` records its background in one reusable Compose graphics layer.
Sibling `GlassSurface` controls sample it at their own coordinates; their text,
icons and input fields are drawn afterward and remain sharp. Background content
gets no access to its own capture source, preventing recursive sampling.

The regular material combines an 18dp blur with subtle 10dp/14dp edge lens
parameters, a neutral wash, a soft shadow and a fine specular edge. These are an
Android approximation of UIKit's system material, not pixel-identical optics.
API 31–32 render the blur without the lens; API 29–30 use an opaque light/dark
surface. No bitmap readback or CPU blur is used. Compose animations respect the
system animator duration scale; UIKit retains system motion/transparency behavior.

The session transcript extends behind its floating composer. Measured composer,
queue, question and navigation-inset height is added to the transcript's bottom
inset so the latest content can scroll completely above the controls. The
keyboard reduces the scene's viewport, rather than covering the composer.

## Verification

Build and run the existing Android unit suite:

```sh
cd apps/android
./gradlew -PzeronSkipCore :app:testDebugUnitTest :app:assembleDebug
```

The core reuse option requires existing generated bindings and native libraries.
For a clean checkout, omit it.

Review demo Sessions, Settings, Search, New session and `chat:chat-veil` in light
and dark appearance. Scroll rows behind navigation; focus the composer, type a
multiline draft, send, and check the last row and jump-to-latest placement. Check
the queue, question panel, large font sizes, and Android 10/11 fallback on devices
before release. Run the iOS Xcode build and existing SessionFlow/TransitionTour
UI tests on a Mac; native iOS effects cannot be exercised on a Linux host.
