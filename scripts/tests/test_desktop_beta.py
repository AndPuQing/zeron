"""Offline desktop Beta packaging and stable release retention regressions."""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def executable(path, contents):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("#!/bin/sh\n" + contents + "\n")
    path.chmod(0o755)


class BetaPackaging(unittest.TestCase):
    @unittest.skipUnless(platform.system() == "Linux" and shutil.which("rsync"), "Linux and rsync required")
    def test_linux_package_installs_matching_ui_and_engine_without_replacing_saved_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "checkout"
            root.mkdir()
            for name in ["scripts/package-linux.sh", "dist/zeron.desktop", "dist/zeron.png",
                         "LICENSE", "THIRD_PARTY_NOTICES.md", "crates/voice/NOTICE.md"]:
                dest = root / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, dest)
            for name in ["crates/ui/assets/fonts/licenses/font.txt", "apps/zgx/fonts/font-OFL.txt",
                         "apps/zgx/ui/app.luau", "apps/zgx/ui/tests/not-shipped.luau",
                         "apps/zgx/ui/.zgx/generated.luau", "apps/zgx/plugins-builtin/test/plugin.json"]:
                dest = root / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_text("fixture")
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "9.9.9"\n')
            version = "9.9.9-beta.1"
            engine = root / "zerun-engine"
            executable(engine, f'# Multi-device controller for coding agents\necho "zerun-engine {version}"')
            binary = root / "apps/zgx/target/release/zerun-experimental"
            executable(binary, f'echo "zerun {version}"')
            shim = Path(temp) / "bin"
            executable(shim / "cargo", f'''[ "$PWD" = '{root}/apps/zgx' ] || exit 7
[ "$ZERUN_BETA_VERSION" = '{version}' ] || exit 8
[ "$ZERUN_ENGINE_VERSION" = '{version}' ] || exit 9''')
            env = {**os.environ, "PATH": f"{shim}:{os.environ['PATH']}",
                   "BETA_VERSION": version, "ZGX_BETA_ENGINE": str(engine)}
            built = subprocess.run(["bash", str(root / "scripts/package-linux.sh")],
                                   env=env, text=True, capture_output=True, timeout=30)
            self.assertEqual(built.returncode, 0, built.stderr)
            tarball, = (root / "target/package").glob("*.tar.gz")
            unpack = Path(temp) / "unpacked"
            with tarfile.open(tarball) as archive:
                archive.extractall(unpack, filter="data")
            package, = unpack.iterdir()
            self.assertTrue((package / "ui/app.luau").is_file())
            self.assertTrue((package / "plugins-builtin/test/plugin.json").is_file())
            self.assertFalse((package / "ui/tests").exists())
            self.assertFalse((package / "ui/.zgx").exists())
            sandbox_home = Path(temp) / "home with spaces"
            saved = sandbox_home / ".zerun/saved-chat.json"
            saved.parent.mkdir(parents=True)
            saved.write_text("saved conversation")
            for _ in range(2):
                installed = subprocess.run(["bash", str(package / "install.sh")],
                    env={**os.environ, "HOME": str(sandbox_home), "XDG_DATA_HOME": str(sandbox_home / ".local/share")},
                    text=True, capture_output=True, timeout=15)
                self.assertEqual(installed.returncode, 0, installed.stderr)
            current = sandbox_home / ".zerun/app/current"
            self.assertEqual(current.resolve().name, version)
            for name, prefix in [("zerun", "zerun"), ("zerun-engine", "zerun-engine")]:
                actual = subprocess.check_output([current / name, "--version"], text=True).strip()
                self.assertEqual(actual, f"{prefix} {version}")
            self.assertEqual(saved.read_text(), "saved conversation")
            launcher = (sandbox_home / ".local/share/applications/zerun.desktop").read_text()
            self.assertIn('/.zerun/app/current/zerun" %u', launcher)

    @unittest.skipUnless(shutil.which("jq"), "jq required")
    def test_betas_do_not_displace_stable_retention_or_enter_its_delete_plan(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            releases = root / "releases.json"
            releases.write_text(json.dumps([
                {"tag_name": tag, "created_at": at, "draft": draft, "prerelease": pre,
                 "assets": [{"name": f"zerun-{tag}-linux-x86_64.tar.gz"}]}
                for tag, at, draft, pre in [
                    ("v1", "2026-01-01", False, False), ("v2", "2026-02-01", False, False),
                    ("v3", "2026-03-01", False, False), ("beta1", "2026-04-01", False, True),
                    ("beta2", "2026-05-01", False, True), ("draft", "2026-06-01", True, False)]
            ]))
            executable(root / "gh", '''while [ "$#" -gt 0 ]; do
  if [ "$1" = --jq ]; then exec jq -c "$2" "$TEST_RELEASES"; fi
  shift
done
exit 1''')
            for mode in [[], ["--full"]]:
                result = subprocess.run(["bash", str(ROOT / "scripts/prune-r2-releases.sh"),
                    "--repo", "example/project", "--keep", "2", *mode], text=True, capture_output=True,
                    env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}", "TEST_RELEASES": str(releases)}, timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("would delete zerun-releases/zerun-v1-linux-x86_64.tar.gz", result.stdout)
                for tag in ["v2", "v3", "beta1", "beta2", "draft"]:
                    self.assertNotIn(f"would delete zerun-releases/zerun-{tag}-", result.stdout)
                self.assertIn("keeping: v2, v3", result.stdout)


if __name__ == "__main__":
    unittest.main()
