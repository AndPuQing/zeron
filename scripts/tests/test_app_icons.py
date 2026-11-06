"""App-store, launcher and packaging contracts; no platform SDK required."""
import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import unittest
import xml.etree.ElementTree as ET

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("app_icons", ROOT / "scripts/make-app-icons.py")
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)
ANDROID = ROOT / icons.ANDROID


class AppIconContracts(unittest.TestCase):
    def test_generated_assets_are_current(self):
        for path, data in icons.generate().items():
            with self.subTest(path=path):
                self.assertTrue(icons.same_asset(ROOT / path, data), path)

    def test_ios_all_appearances_are_square_and_opaque(self):
        catalog = ROOT / icons.IOS
        entries = json.loads((catalog / "Contents.json").read_text())["images"]
        appearances = set()
        for entry in entries:
            image = Image.open(io.BytesIO((catalog / entry["filename"]).read_bytes()))
            self.assertEqual(image.size, (1024, 1024))
            self.assertEqual(image.mode, "RGB")
            self.assertNotIn("transparency", image.info)
            appearance = entry.get("appearances", [{"value": "light"}])[0]["value"]
            appearances.add(appearance)
            if appearance == "tinted":
                r, g, b = image.split()
                self.assertIsNone(ImageChops.difference(r, g).getbbox())
                self.assertIsNone(ImageChops.difference(g, b).getbbox())
                self.assertEqual(image.getpixel((0, 0)), (0, 0, 0))
        self.assertEqual(appearances, {"light", "dark", "tinted"})

    def test_macos_keeps_the_original_alpha_and_margin(self):
        image = Image.open(ROOT / "dist/macos/icon-1024.png")
        alpha = image.getchannel("A")
        # Captured from the original shipped icon before changing its artwork.
        self.assertEqual(hashlib.sha256(alpha.tobytes()).hexdigest(),
                         "002200b38cc4c1af8ce9c4fecafee431a6f69ec1d09afe3d03eb1affbe0d6cf8")
        self.assertEqual(alpha.getbbox(), (72, 82, 952, 962))
        self.assertEqual(alpha.getpixel((0, 0)), 0)
        self.assertEqual(alpha.getpixel((512, 512)), 255)

    def test_windows_contains_all_launcher_sizes_with_transparency(self):
        image = Image.open(io.BytesIO((ROOT / "dist/windows/zeron.ico").read_bytes()))
        self.assertEqual(image.ico.sizes(), {(s, s) for s in icons.ICO_SIZES})
        for size in image.ico.sizes():
            frame = image.ico.getimage(size)
            self.assertEqual(frame.size, size)
            self.assertEqual(frame.getpixel((0, 0))[3], 0)

    def test_android_layers_have_safe_padding_and_separate_monochrome(self):
        namespace = "{http://schemas.android.com/apk/res/android}drawable"
        root = ET.parse(ANDROID / "mipmap-anydpi/ic_launcher.xml").getroot()
        self.assertEqual(root.find("foreground").get(namespace), "@drawable/ic_launcher_portrait")
        self.assertEqual(root.find("monochrome").get(namespace), "@drawable/ic_launcher_monochrome")
        for bucket, size in [("mdpi", 108), ("hdpi", 162), ("xhdpi", 216),
                             ("xxhdpi", 324), ("xxxhdpi", 432)]:
            for name in ["ic_launcher_portrait", "ic_launcher_monochrome"]:
                image = Image.open(ANDROID / f"drawable-{bucket}/{name}.png")
                self.assertEqual(image.size, (size, size))
                self.assertEqual(image.mode, "RGBA")
                x0, y0, x1, y1 = image.getchannel("A").point(lambda v: 255 if v >= 8 else 0).getbbox()
                # Lanczos can produce faint antialiasing just outside the safe region.
                pad = size * 21 / 108 - 2
                self.assertGreaterEqual(min(x0, y0), pad)
                self.assertLessEqual(max(x1, y1), size - pad)
                if name.endswith("monochrome"):
                    r, g, b, _ = image.split()
                    self.assertIsNone(ImageChops.difference(r, g).getbbox())
                    self.assertIsNone(ImageChops.difference(g, b).getbbox())
        screen = (ROOT / "apps/android/app/src/main/java/sh/zeron/android/ui/SignInScreen.kt").read_text()
        self.assertIn("R.drawable.zerun_app_icon", screen)
        for qualifier in ["drawable-nodpi", "drawable-night-nodpi"]:
            self.assertEqual(Image.open(io.BytesIO((ANDROID / qualifier / "zerun_app_icon.png").read_bytes())).mode, "RGB")

    def test_website_icon_links_and_svg_fallback_are_self_contained(self):
        web = ROOT / icons.WEB
        html = (web / "index.html").read_text()
        for link in re.findall(r'<link[^>]+rel="(?:icon|apple-touch-icon)"[^>]+>', html):
            path = re.search(r'href="([^"]+)"', link)[1]
            self.assertTrue((web / path.lstrip("/")).is_file(), path)
        root = ET.parse(web / "favicon.svg").getroot()
        self.assertIn("prefers-color-scheme:dark", root.find("{http://www.w3.org/2000/svg}style").text)
        images = root.findall("{http://www.w3.org/2000/svg}image")
        self.assertEqual(len(images), 2)
        for image in images:
            payload = image.get("href").removeprefix("data:image/png;base64,")
            self.assertEqual(Image.open(io.BytesIO(base64.b64decode(payload))).size, (128, 128))

    def test_template_icons_keep_current_color_and_generated_inline_source(self):
        source = (ROOT / "dist/artwork/statue-mark.svg").read_text()
        self.assertIn('fill="currentColor"', source)
        self.assertNotIn("data:image", source)
        ET.fromstring(source)
        inline = (ROOT / "apps/zgx/ui/ds/icon_svgs.luau").read_text()
        self.assertIn('["zeron-logo"] = [==[' + source.strip() + ']==],', inline)


if __name__ == "__main__":
    unittest.main()
