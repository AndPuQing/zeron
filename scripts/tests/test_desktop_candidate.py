"""Stage and apply the desktop candidate over released GPUI and Beta packages."""
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import tomllib


def main():
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Windows":
        suffix = "windows-x86_64.zip"
    elif system == "Darwin":
        suffix = "macos-arm64-app.tar.gz"
    else:
        suffix = f"linux-{machine}.tar.gz"

    with open("Cargo.toml", "rb") as manifest:
        version = tomllib.load(manifest)["workspace"]["package"]["version"]
    package_dir = Path("target/package").resolve()
    env = {
        **os.environ,
        "ZERUN_CANDIDATE_PACKAGE": str(package_dir / f"zerun-{version}-{suffix}"),
    }
    if system == "Windows":
        env["ZERUN_CANDIDATE_EXE"] = str(package_dir / f"zerun-{version}-windows-x86_64.exe")

    with tempfile.TemporaryDirectory() as temporary:
        for key, tag, previous in [
            ("ZERUN_REFERENCE_GPUI", "v0.3.30", "0.3.30"),
            ("ZERUN_REFERENCE_BETA", "beta-v0.3.31-beta.1", "0.3.31-beta.1"),
        ]:
            name = f"zerun-{previous}-{suffix}"
            subprocess.run(
                ["gh", "release", "download", tag, "--repo", os.environ["GITHUB_REPOSITORY"],
                 "--pattern", name, "--dir", temporary],
                check=True,
            )
            env[key] = str(Path(temporary) / name)
        subprocess.run(
            ["cargo", "test", "--release", "--locked", "-p", "zeron-update", "--lib",
             "desktop_candidate_upgrades", "--", "--ignored", "--nocapture"],
            check=True,
            env=env,
        )


if __name__ == "__main__":
    main()
