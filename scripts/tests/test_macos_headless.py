"""Offline CLI installation and macOS standalone signing regressions."""
import os
import json
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HeadlessInstall(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home with spaces"
        self.home.mkdir()
        self.shim = self.root / "shim"
        self.shim.mkdir()
        self.site = self.root / "site" / "releases"
        self.site.mkdir(parents=True)
        self.write_exe("uname", 'case "$1" in -s) echo Darwin;; -m) echo "${TEST_ARCH:-arm64}";; esac')
        self.write_exe("sysctl", 'echo "${TEST_ARM:-1}"')
        self.write_exe("launchctl", '[ "${TEST_GUI:-1}" = 1 ]')
        self.write_exe("systemctl", 'echo unexpected-systemd >> "$HOME/unexpected"; exit 1')
        self.env = {
            "HOME": str(self.home), "USER": "tester",
            "PATH": f"{self.shim}:/usr/bin:/bin", "ZERUN_BASE_URL": self.site.parent.as_uri(),
            "TMPDIR": str(self.root),
        }
        self.release("9.9.9")

    def write_exe(self, name, body):
        path = self.shim / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)

    def release(self, version, broken=False):
        name = f"zerun-{version}-macos-arm64"
        package = self.root / name
        package.mkdir(exist_ok=True)
        binary = package / "zerun"
        body = 'exit 1' if broken else f'''case "$*" in
--version) echo "zerun {version}";;
"daemon install") printf '%s\\n' "$0" >> "$HOME/daemon-installs";;
*) exit 1;;
esac'''
        binary.write_text("#!/bin/sh\n" + body + "\n")
        binary.chmod(0o755)
        with tarfile.open(self.site / (name + ".tar.gz"), "w:gz") as archive:
            archive.add(package, arcname=name)
        (self.site / "latest.txt").write_text(version)

    def install(self, **extra):
        return subprocess.run(
            ["sh", str(ROOT / "edge/src/install.sh")], env={**self.env, **extra},
            text=True, capture_output=True, timeout=20,
        )

    def test_install_reinstall_and_upgrade_use_cli_service_only(self):
        state = self.home / ".zerun" / "keep.json"
        state.parent.mkdir()
        state.write_text("saved state")
        for version in ["9.9.9", "9.9.9", "9.9.10"]:
            if version == "9.9.10":
                self.release(version)
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((self.home / ".zerun/app/current").resolve().name, version)
            self.assertEqual((self.home / ".local/bin/zerun").resolve().parent.name, version)
        installs = (self.home / "daemon-installs").read_text().splitlines()
        self.assertEqual(installs, [str(self.home / ".zerun/app/current/zerun")] * 3)
        self.assertEqual(state.read_text(), "saved state")
        self.assertFalse((self.home / ".local/share/applications").exists())
        self.assertFalse((self.home / "unexpected").exists())

    def test_ssh_without_gui_installs_cli_with_manual_instructions(self):
        result = self.install(TEST_GUI="0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SSH-only", result.stdout)
        self.assertTrue((self.home / ".local/bin/zerun").exists())
        self.assertFalse((self.home / "daemon-installs").exists())

    def test_rosetta_downloads_arm64_and_intel_is_rejected_before_install(self):
        result = self.install(TEST_ARCH="x86_64", TEST_ARM="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        previous = (self.home / "daemon-installs").read_text()
        result = self.install(TEST_ARCH="x86_64", TEST_ARM="0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Apple Silicon", result.stderr)
        self.assertEqual((self.home / "daemon-installs").read_text(), previous)

    def test_broken_download_keeps_current_and_does_not_restart(self):
        self.assertEqual(self.install().returncode, 0)
        previous = (self.home / "daemon-installs").read_text()
        self.release("9.9.10", broken=True)
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("ALSA", result.stderr)
        self.assertEqual((self.home / ".zerun/app/current").resolve().name, "9.9.9")
        self.assertEqual((self.home / "daemon-installs").read_text(), previous)
        self.assertEqual(sorted(p.name for p in (self.home / ".zerun/app").iterdir()),
                         ["9.9.9", "current"])
        # A fixed release replaces nothing that was left behind and installs cleanly.
        self.release("9.9.10")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.home / ".zerun/app/current").resolve().name, "9.9.10")

    def test_existing_version_dir_that_fails_its_check_is_downloaded_again(self):
        broken = self.home / ".zerun/app/9.9.9"
        broken.mkdir(parents=True)
        (broken / "zerun").write_text("#!/bin/sh\nexit 1\n")
        (broken / "zerun").chmod(0o755)
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("failed its check", result.stdout)
        self.assertEqual(subprocess.check_output([str(broken / "zerun"), "--version"], text=True).strip(),
                         "zerun 9.9.9")
        self.assertEqual(sorted(p.name for p in broken.parent.iterdir()), ["9.9.9", "current"])

    @unittest.skipUnless(shutil.which("jq"), "jq required for R2 prune plan")
    def test_prune_keeps_current_cli_and_removes_old_cli_artifacts(self):
        releases = [
            {"tag": "v9.9.9", "at": "2026-10-01", "assets": ["zerun-9.9.9-macos-arm64.tar.gz"]},
            {"tag": "v9.9.10", "at": "2026-10-02", "assets": [
                "zerun-9.9.10-macos-arm64.tar.gz", "zerun-9.9.10-macos-arm64-app.tar.gz",
                "zerun-9.9.10-macos-arm64.dmg"]},
        ]
        fixture = self.root / "releases.jsonl"
        fixture.write_text("\n".join(json.dumps(row) for row in releases))
        self.write_exe("gh", 'cat "$TEST_RELEASES"')
        (self.shim / "jq").symlink_to(shutil.which("jq"))
        for mode in [[], ["--full"]]:
            result = subprocess.run([
                "bash", str(ROOT / "scripts/prune-r2-releases.sh"),
                "--repo", "example/project", "--keep", "1", *mode,
            ], env={**self.env, "TEST_RELEASES": str(fixture)}, text=True, capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("zerun-9.9.9-macos-arm64.tar.gz", result.stdout)
            self.assertNotIn("zerun-9.9.10-macos-arm64.tar.gz", result.stdout)
            self.assertNotIn("zerun-9.9.10-macos-arm64-app.tar.gz", result.stdout)

    @unittest.skipUnless(platform.system() == "Darwin", "macOS codesign required")
    def test_packaged_binary_is_signed_standalone_and_runs_after_extraction(self):
        source = self.root / "version.c"
        source.write_text('#include <stdio.h>\nint main(void) { puts("zerun 9.9.9"); return 0; }\n')
        binary = self.root / "zerun"
        subprocess.run(["cc", str(source), "-o", str(binary)], check=True, capture_output=True)
        env = {k: v for k, v in os.environ.items() if k not in (
            "CODESIGN_IDENTITY", "NOTARY_KEY_PATH", "NOTARY_KEY_ID", "NOTARY_ISSUER_ID")}
        out = self.root / "output"
        subprocess.run(["bash", str(ROOT / "scripts/package-macos-headless.sh"),
                        str(binary), str(out), "9.9.9"], check=True, env=env, capture_output=True)
        tarball, = out.glob("*.tar.gz")
        unpacked = self.root / "unpacked"
        unpacked.mkdir()
        subprocess.run(["tar", "-xzf", str(tarball), "-C", str(unpacked), "--strip-components=1"], check=True)
        executable = unpacked / "zerun"
        subprocess.run(["codesign", "--verify", "--strict", str(executable)], check=True, capture_output=True)
        signature = subprocess.run(["codesign", "-dv", str(executable)], text=True, capture_output=True, check=True)
        self.assertIn("Identifier=work.puqing.zerun.cli", signature.stderr)
        self.assertEqual(subprocess.check_output([str(executable), "--version"], text=True).strip(), "zerun 9.9.9")
        self.assertTrue((unpacked / "LICENSE").exists())
        self.assertTrue((unpacked / "licenses/fonts").is_dir())
        self.assertFalse(list(out.glob(".headless.*")))


if __name__ == "__main__":
    unittest.main()
