#!/usr/bin/env python3
"""Generate app icons from the approved artwork. Requires Pillow 13.0.0.

    uv run --with pillow==13.0.0 python scripts/make-app-icons.py
    uv run --with pillow==13.0.0 python scripts/make-app-icons.py --check

No model request or platform SDK is needed. Sources live in dist/artwork.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import re
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps

ROOT = Path(__file__).resolve().parents[1]
ART = ROOT / "dist/artwork"
IOS = "apps/ios/Zeron/Assets.xcassets/AppIcon.appiconset"
ANDROID = "apps/android/app/src/main/res"
WEB = "apps/landing/public"
SIZE = 1024
LANCZOS = Image.Resampling.LANCZOS
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
# Remove the generated preview's outer margin; do not stretch the portrait.
ART_CROP = (122, 121, 1132, 1131)
# The desktop's existing opaque tile is 824px with a 100px inset.
MAC_TILE = (100, 100, 924, 924)


def png(image: Image.Image) -> bytes:
    output = io.BytesIO()
    image.save(output, format="PNG", optimize=True)
    return output.getvalue()


def gradient(top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    strip = Image.new("RGB", (1, SIZE))
    strip.putdata([tuple(round(a + (b - a) * y / (SIZE - 1))
                         for a, b in zip(top, bottom)) for y in range(SIZE)])
    return strip.resize((SIZE, SIZE))


def rounded(image: Image.Image, radius: int = 224) -> Image.Image:
    # Supersample only the silhouette, so small outputs have clean alpha edges.
    mask = Image.new("L", (SIZE * 2, SIZE * 2))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, SIZE * 2 - 1, SIZE * 2 - 1),
                                           radius=radius * 2, fill=255)
    out = image.convert("RGBA")
    out.putalpha(mask.resize((SIZE, SIZE), LANCZOS))
    return out


def svg_image(image: Image.Image, dark: Image.Image | None = None) -> bytes:
    def tag(image: Image.Image, klass: str) -> str:
        encoded = base64.b64encode(png(image)).decode("ascii")
        return (f'<image class="{klass}" width="1024" height="1024" '
                f'href="data:image/png;base64,{encoded}"/>')
    content = tag(image, "light")
    if dark is not None:
        content = ('<style>.dark{display:none}@media(prefers-color-scheme:dark)'
                   '{.light{display:none}.dark{display:inline}}</style>'
                   + content + tag(dark, "dark"))
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">'
            + content + '</svg>\n').encode()


def same_asset(path: Path, generated: bytes) -> bool:
    """Compare pixels, not platform-dependent PNG compression or metadata."""
    if not path.is_file():
        return False
    actual = path.read_bytes()
    if actual == generated:
        return True
    if path.suffix in {".png", ".ico"}:
        a, b = Image.open(io.BytesIO(actual)), Image.open(io.BytesIO(generated))
        if path.suffix == ".ico":
            if a.ico.sizes() != b.ico.sizes():
                return False
            return all(a.ico.getimage(size).convert("RGBA").tobytes()
                       == b.ico.getimage(size).convert("RGBA").tobytes()
                       for size in a.ico.sizes())
        return a.size == b.size and a.convert("RGBA").tobytes() == b.convert("RGBA").tobytes()
    if path.suffix == ".svg":
        def normalize(data: bytes) -> bytes:
            def digest(match):
                image = Image.open(io.BytesIO(base64.b64decode(match[1])))
                pixels = str(image.size).encode() + image.convert("RGBA").tobytes()
                return hashlib.sha256(pixels).hexdigest().encode()
            return re.sub(rb'data:image/png;base64,([A-Za-z0-9+/=]+)', digest, data)
        return normalize(actual) == normalize(generated)
    return False


def generate() -> dict[str, bytes]:
    source = Image.open(ART / "statue-orange.png").convert("RGB")
    if source.size != (1254, 1254):
        raise ValueError("statue-orange.png must retain its approved 1254px canvas")
    artwork = source.crop(ART_CROP).resize((SIZE, SIZE), LANCZOS)
    alpha = Image.open(ART / "statue-alpha.png").convert("L")
    mac_alpha = Image.open(ART / "macos-alpha.png").convert("L")
    if alpha.size != (SIZE, SIZE) or mac_alpha.size != (SIZE, SIZE):
        raise ValueError("Source masks must be 1024px square")

    portrait = artwork.convert("RGBA")
    portrait.putalpha(alpha)
    light = gradient((252, 250, 246), (230, 224, 215)).convert("RGBA")
    # A restrained cast shadow on the light background, below the portrait.
    shadow = Image.new("RGBA", (SIZE, SIZE), (64, 36, 15, 0))
    shadow.putalpha(alpha.filter(ImageFilter.GaussianBlur(16)).point(lambda v: v // 8))
    light.alpha_composite(shadow, (0, 9))
    light.alpha_composite(portrait)
    light = light.convert("RGB")

    # Keep the approved patterned background in dark mode. Fill only the old
    # rounded preview corners, so iOS/Android receive full-bleed square artwork.
    corner = Image.new("L", (SIZE, SIZE))
    ImageDraw.Draw(corner).rounded_rectangle((-4, -4, SIZE + 3, SIZE + 3),
                                             radius=215, fill=255)
    corner = corner.filter(ImageFilter.GaussianBlur(8))
    corner = ImageChops.lighter(corner, alpha)
    dark = gradient((35, 15, 6), (12, 7, 4))
    dark.paste(artwork, (0, 0), corner)

    # System tinting uses a black-backed grayscale portrait, not a pre-tinted
    # orange tile. Preserve the facet detail and remove background symbols.
    luminance = ImageOps.grayscale(artwork).point(lambda v: round((v / 255) ** 0.65 * 255))
    tinted = ImageChops.multiply(luminance, alpha).convert("RGB")

    # Preserve the exact macOS alpha, including its soft outer shadow.
    mac = Image.new("RGBA", (SIZE, SIZE))
    mac.paste(dark.resize((824, 824), LANCZOS), MAC_TILE[:2])
    mac.putalpha(mac_alpha)
    ico = io.BytesIO()
    mac.save(ico, format="ICO", sizes=[(s, s) for s in ICO_SIZES])

    outputs = {
        "dist/zeron.png": png(dark),
        "dist/zeron.svg": svg_image(rounded(dark)),
        "dist/macos/icon-1024.png": png(mac),
        "dist/windows/zeron.ico": ico.getvalue(),
        f"{IOS}/AppIcon1024.png": png(light),
        f"{IOS}/AppIcon1024-dark.png": png(dark),
        f"{IOS}/AppIcon1024-tinted.png": png(tinted),
        f"{WEB}/assets/zeron-app-icon.png": png(light.resize((180, 180), LANCZOS)),
        f"{WEB}/assets/zeron-favicon-light.png": png(rounded(light).resize((128, 128), LANCZOS)),
        f"{WEB}/assets/zeron-favicon-dark.png": png(rounded(dark).resize((128, 128), LANCZOS)),
        f"{WEB}/favicon.svg": svg_image(rounded(light).resize((128, 128), LANCZOS),
                                        rounded(dark).resize((128, 128), LANCZOS)),
        "apps/zgx/ui/assets/favicons/zeron.png": png(rounded(dark).resize((64, 64), LANCZOS)),
    }

    # A traced single-ink portrait is used by template/tint-only UI surfaces.
    # Keep vector consumers as vectors rather than embedding a tinted bitmap.
    mark = (ART / "statue-mark.svg").read_text()
    for path in ["crates/ui/assets/icons/zeron-logo.svg",
                 "apps/ios/Zeron/Assets.xcassets/zeron-logo.imageset/zeron-logo.svg",
                 "apps/zgx/ui/assets/icons/ui/zeron-logo.svg"]:
        outputs[path] = mark.encode()
    outputs["apps/android/icons/zeron.svg"] = mark.replace("currentColor", "#000000").encode()
    inline = "apps/zgx/ui/ds/icon_svgs.luau"
    content = (ROOT / inline).read_text()
    content, count = re.subn(r'\["zeron-logo"\] = \[==\[.*?\]==\],',
                            lambda _: '["zeron-logo"] = [==[' + mark.strip() + ']==],',
                            content, flags=re.S)
    if count != 1:
        raise ValueError("Expected one inline zeron-logo in the experimental UI")
    outputs[inline] = content.encode()

    # 108dp adaptive layers at xxxhdpi (432px). Portrait fits the central
    # 66dp safe region; the OS owns all circle/squircle/other launcher masks.
    adaptive = Image.new("RGBA", (432, 432))
    adaptive.alpha_composite(portrait.resize((264, 264), LANCZOS), (84, 84))
    mono = Image.new("RGBA", (432, 432))
    mono_alpha = Image.new("L", (432, 432))
    mono_alpha.paste(ImageOps.grayscale(tinted).resize((264, 264), LANCZOS), (84, 84))
    mono.paste((255, 255, 255, 255), (0, 0, 432, 432))
    mono.putalpha(mono_alpha)
    for bucket, side in [("mdpi", 108), ("hdpi", 162), ("xhdpi", 216),
                         ("xxhdpi", 324), ("xxxhdpi", 432)]:
        outputs[f"{ANDROID}/drawable-{bucket}/ic_launcher_portrait.png"] = png(adaptive.resize((side, side), LANCZOS))
        outputs[f"{ANDROID}/drawable-{bucket}/ic_launcher_monochrome.png"] = png(mono.resize((side, side), LANCZOS))
    # Sign-in must not show an adaptive layer's extra launcher padding.
    outputs[f"{ANDROID}/drawable-nodpi/zerun_app_icon.png"] = png(light.resize((512, 512), LANCZOS))
    outputs[f"{ANDROID}/drawable-night-nodpi/zerun_app_icon.png"] = png(dark.resize((512, 512), LANCZOS))
    return outputs


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check committed assets without writing")
    args = parser.parse_args()
    stale = []
    for relative, data in generate().items():
        path = ROOT / relative
        if args.check:
            if not same_asset(path, data):
                stale.append(relative)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("Regenerate app icons:\n" + "\n".join(stale))
        return 1
    print("App icons are up to date." if args.check else "Generated app icons.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
