//! Use dsh's installed persistence API so compressed logs and released formats
//! remain the provider's responsibility. The helper never boots a model/profile.
use super::AcpHarness;
use crate::{
    HarnessError,
    process::{Command, Stdio, owned},
    saved_sessions::*,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeron_proto::HarnessId;

fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}
impl AcpHarness {
    fn dsh_saved_home(&self) -> Result<PathBuf, HarnessError> {
        if self.spec.id != HarnessId::Dsh {
            return Err(unsupported());
        }
        Ok(self
            .saved_home
            .clone()
            .unwrap_or_else(|| {
                match self
                    .environment
                    .entries
                    .iter()
                    .find(|(name, _)| crate::environment::normalized_name(name) == "DSH_HOME")
                    .map(|(_, entry)| entry)
                {
                    Some(crate::environment::EnvironmentEntry::Set { value })
                        if !value.is_empty() =>
                    {
                        PathBuf::from(value)
                    }
                    Some(
                        crate::environment::EnvironmentEntry::Unset
                        | crate::environment::EnvironmentEntry::Set { .. },
                    ) => crate::executable::home_or_current_dir().join(".dsh"),
                    _ => super::dsh_home(),
                }
            })
            .canonicalize()?)
    }
    fn dsh_saved_root(&self) -> Result<PathBuf, HarnessError> {
        Ok(self.dsh_saved_home()?.join("sessions").canonicalize()?)
    }
    pub(super) fn dsh_saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::Dsh, &self.dsh_saved_root()?))
    }
    pub(super) async fn dsh_saved_list(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        match self.dsh_saved_root() {
            Err(HarnessError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedSessionPage::default());
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        self.dsh_saved_call("list", cursor, None).await
    }
    pub(super) async fn dsh_saved_call<T: serde::de::DeserializeOwned>(
        &self,
        action: &str,
        cursor: Option<&str>,
        session: Option<&SavedSession>,
    ) -> Result<T, HarnessError> {
        let identity = self.dsh_saved_store_id()?;
        if session.is_some_and(|s| s.harness != HarnessId::Dsh || s.store_id != identity) {
            return Err(error("The dsh history location changed. Scan again."));
        }
        let home = self.dsh_saved_home()?;
        let profile = super::dsh_profile();
        let runtime = self.dsh_helper_runtime()?;
        let input = serde_json::to_vec(&json!({"action": action, "home": home, "root": self.dsh_saved_root()?, "profile": profile, "identity": identity, "cursor": cursor, "session": session, "packages": runtime.packages})).map_err(|e| error(e.to_string()))?;
        helper(&runtime, &input, &self.environment).await
    }
    /// What reads the history: the desktop app's own runtime when `dsh` is its
    /// launcher, else Node.js over the packages the `dsh` package ships (or,
    /// for releases that kept them there, the profile packages).
    fn dsh_helper_runtime(&self) -> Result<HelperRuntime, HarnessError> {
        let dsh = match self.resolve_launch() {
            Ok(super::Launch::Program(dsh, _)) => Some(dsh),
            _ => None,
        };
        if let Some(runtime) = dsh.as_deref().and_then(desktop_app_runtime) {
            return Ok(runtime);
        }
        // An npm global install keeps `dsh` beside the `node` that runs it.
        let node = dsh
            .as_deref()
            .and_then(Path::parent)
            .map(|bin| bin.join(if cfg!(windows) { "node.exe" } else { "node" }))
            .filter(|node| node.is_file())
            .or_else(|| crate::executable::find_on_paths("node", vec![]))
            .ok_or_else(|| error("Node.js is required to read dsh's native history."))?;
        Ok(HelperRuntime {
            program: node,
            electron: false,
            packages: dsh.as_deref().and_then(npm_package_manifest),
        })
    }
}

/// The `package.json` of the `@deepseek-ai/dsh` package an npm (or bun)
/// install's `dsh` runs from; its dependencies hold the persistence packages.
fn npm_package_manifest(dsh: &Path) -> Option<PathBuf> {
    let entry = dsh.canonicalize().ok()?;
    entry.ancestors().skip(1).take(4).find_map(|dir| {
        let manifest = dir.join("package.json");
        let text = std::fs::read_to_string(&manifest).ok()?;
        let name = serde_json::from_str::<serde_json::Value>(&text).ok()?["name"].clone();
        (name == "@deepseek-ai/dsh").then_some(manifest)
    })
}

/// What runs the helper script.
struct HelperRuntime {
    program: PathBuf,
    /// An Electron binary run as Node (`ELECTRON_RUN_AS_NODE`), which can read
    /// packages inside an `app.asar` archive.
    electron: bool,
    /// The `package.json` dsh's packages resolve from, when not the profiles'.
    packages: Option<PathBuf>,
}

/// The macOS desktop app's `dsh` is a launcher at
/// `<App>.app/Contents/Resources/runtime/cli/bin/dsh` that runs the app's
/// Electron binary as Node over `Resources/app.asar/dsh/node_modules`, where
/// every dsh package lives. Plain Node can't read that archive.
fn desktop_app_runtime(dsh: &Path) -> Option<HelperRuntime> {
    let launcher = dsh.canonicalize().ok()?;
    if !launcher.ends_with("Contents/Resources/runtime/cli/bin/dsh") {
        return None;
    }
    let resources = launcher.ancestors().nth(4)?;
    let contents = resources.parent()?;
    let plist = std::fs::read_to_string(contents.join("Info.plist")).ok()?;
    let program = contents
        .join("MacOS")
        .join(plist_string(&plist, "CFBundleExecutable")?);
    let archive = resources.join("app.asar");
    (program.is_file() && archive.is_file()).then(|| HelperRuntime {
        program,
        electron: true,
        packages: Some(archive.join("dsh").join("package.json")),
    })
}

/// A `<string>` value from an XML property list.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let value = plist
        .split(&format!("<key>{key}</key>"))
        .nth(1)?
        .trim_start()
        .strip_prefix("<string>")?;
    let value = &value[..value.find("</string>")?];
    Some(
        value
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
    .filter(|value| !value.is_empty())
}
async fn helper<T: serde::de::DeserializeOwned>(
    runtime: &HelperRuntime,
    input: &[u8],
    environment: &crate::environment::EnvironmentSnapshot,
) -> Result<T, HarnessError> {
    let mut command = Command::new(&runtime.program);
    command
        .args([
            "--max-old-space-size=256",
            "--input-type=module",
            "-e",
            include_str!("dsh_saved.mjs"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    owned::configure(&mut command);
    environment.apply(&mut command);
    if runtime.electron {
        command.env("ELECTRON_RUN_AS_NODE", "1");
    }
    let mut child = owned::Child::new(command.spawn()?);
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(120), async {
        stdin.write_all(input).await?;
        stdin.shutdown().await?;
        drop(stdin);
        let (output, errors) = tokio::try_join!(
            async {
                let mut b = Vec::new();
                (&mut stdout)
                    .take((MAX_HISTORY_BYTES + 65537) as u64)
                    .read_to_end(&mut b)
                    .await?;
                Ok::<_, std::io::Error>(b)
            },
            async {
                let mut b = Vec::new();
                (&mut stderr).take(65537).read_to_end(&mut b).await?;
                Ok::<_, std::io::Error>(b)
            }
        )?;
        if output.len() > MAX_HISTORY_BYTES + 65536 || errors.len() > 65536 {
            return Err(error("dsh import response exceeds the size limit."));
        }
        if !child.wait().await?.success() {
            return Err(error(format!(
                "dsh history: {}",
                String::from_utf8_lossy(&errors)
            )));
        }
        serde_json::from_slice(&output)
            .map_err(|e| error(format!("Invalid dsh import response: {e}")))
    })
    .await
    .unwrap_or_else(|_| Err(error("dsh history read timed out.")));
    child.shutdown(Duration::from_millis(100)).await;
    outcome
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::Harness;
    use std::fs;
    fn snapshot(root: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                snapshot(&path, files);
            } else {
                files.push((path.clone(), fs::read(path).unwrap()));
            }
        }
    }
    /// Runs the fixture the way production runs the helper: Node beside an
    /// npm install's profiles, or the desktop app's runtime over its archive.
    async fn fixture(
        runtime: &HelperRuntime,
        home: &Path,
        action: &str,
        compression: &str,
        copy: &str,
    ) {
        let script = home.join("fixture.mjs");
        fs::write(&script, include_str!("../../tests/fixtures/dsh_saved.mjs")).unwrap();
        let mut command = tokio::process::Command::new(&runtime.program);
        if runtime.electron {
            command.env("ELECTRON_RUN_AS_NODE", "1");
        }
        let output = command
            .arg(script)
            .arg(home)
            .arg(action)
            .arg(compression)
            .arg(copy)
            .arg(runtime.packages.as_deref().unwrap_or(Path::new("")))
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[tokio::test]
    async fn no_history_does_not_require_profile_packages() {
        let dir = tempfile::tempdir().unwrap();
        let page = AcpHarness::dsh()
            .with_saved_session_home(dir.path())
            .saved_sessions(None)
            .await
            .unwrap();
        assert!(page.sessions.is_empty() && page.next_cursor.is_none());
    }
    /// A minimal desktop-app bundle: Info.plist, Electron binary, archive and
    /// the CLI launcher, reached through a PATH-style symlink.
    fn desktop_app(root: &Path, with_archive: bool) -> PathBuf {
        let contents = root.join("DeepSeek Harness.app/Contents");
        let launcher = contents.join("Resources/runtime/cli/bin/dsh");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(&launcher, "#!/bin/sh\n").unwrap();
        fs::write(contents.join("MacOS/DeepSeek Harness"), "").unwrap();
        fs::write(
            contents.join("Info.plist"),
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict>\n\
             <key>CFBundleIdentifier</key><string>example.dsh</string>\n\
             <key>CFBundleExecutable</key>\n\t<string>DeepSeek Harness</string>\n\
             </dict></plist>\n",
        )
        .unwrap();
        if with_archive {
            fs::write(contents.join("Resources/app.asar"), "").unwrap();
        }
        let link = root.join("bin/dsh");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&launcher, &link).unwrap();
        link
    }
    #[test]
    fn desktop_app_launcher_runs_the_helper_on_the_app_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let runtime = desktop_app_runtime(&desktop_app(&root, true)).unwrap();
        let contents = root.join("DeepSeek Harness.app/Contents");
        assert!(runtime.electron);
        assert_eq!(runtime.program, contents.join("MacOS/DeepSeek Harness"));
        assert_eq!(
            runtime.packages.unwrap(),
            contents.join("Resources/app.asar/dsh/package.json")
        );
    }
    #[test]
    fn npm_installs_and_incomplete_bundles_keep_node() {
        let dir = tempfile::tempdir().unwrap();
        let npm = dir.path().join("lib/node_modules/@deepseek-ai/dsh/bin/dsh");
        fs::create_dir_all(npm.parent().unwrap()).unwrap();
        fs::write(&npm, "#!/usr/bin/env node\n").unwrap();
        assert!(desktop_app_runtime(&npm).is_none());
        let incomplete = tempfile::tempdir().unwrap();
        assert!(desktop_app_runtime(&desktop_app(incomplete.path(), false)).is_none());
    }
    #[test]
    fn npm_installs_resolve_packages_from_the_dsh_package() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().canonicalize().unwrap();
        let package = prefix.join("lib/node_modules/@deepseek-ai/dsh");
        fs::create_dir_all(package.join("lib")).unwrap();
        fs::write(package.join("lib/bin.js"), "#!/usr/bin/env node\n").unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"name":"@deepseek-ai/dsh","bin":{"dsh":"lib/bin.js"}}"#,
        )
        .unwrap();
        fs::create_dir_all(prefix.join("bin")).unwrap();
        let link = prefix.join("bin/dsh");
        std::os::unix::fs::symlink(package.join("lib/bin.js"), &link).unwrap();
        assert_eq!(
            npm_package_manifest(&link),
            Some(package.join("package.json"))
        );
        // Another package's bin is not dsh's.
        fs::write(package.join("package.json"), r#"{"name":"other"}"#).unwrap();
        assert_eq!(npm_package_manifest(&link), None);
    }
    #[test]
    fn plist_strings_are_unescaped() {
        let plist = "<key>CFBundleExecutable</key><string>A &amp; B</string>";
        assert_eq!(
            plist_string(plist, "CFBundleExecutable").as_deref(),
            Some("A & B")
        );
        assert_eq!(plist_string(plist, "CFBundleName"), None);
    }
    #[tokio::test]
    #[ignore = "requires an installed dsh (desktop app or npm), or ZERON_DSH_TEST_PACKAGES=<profiles directory>"]
    async fn native_plain_and_compressed_copies_preserve_source_tools_and_resume_independently() {
        let packages = std::env::var_os("ZERON_DSH_TEST_PACKAGES");
        let runtime = AcpHarness::dsh().dsh_helper_runtime().unwrap();
        assert!(
            packages.is_some() || runtime.packages.is_some(),
            "set ZERON_DSH_TEST_PACKAGES or install dsh"
        );
        for compression in ["none", "zstd"] {
            let home = tempfile::tempdir().unwrap();
            fs::create_dir(home.path().join("sessions")).unwrap();
            if let Some(packages) = &packages {
                std::os::unix::fs::symlink(packages, home.path().join("profiles")).unwrap();
            }
            fixture(&runtime, home.path(), "create", compression, "").await;
            let mut originals = vec![];
            snapshot(&home.path().join("sessions"), &mut originals);
            let harness = AcpHarness::dsh().with_saved_session_home(home.path());
            let page = harness.saved_sessions(None).await.unwrap();
            assert_eq!(page.sessions.len(), 2);
            let busy = page.sessions.iter().find(|s| s.running).unwrap();
            assert!(harness.copy_saved_session(busy).await.is_err());
            let source = page.sessions.iter().find(|s| !s.running).unwrap();
            assert_eq!(source.model.as_deref(), Some(r#"["fixture","test"]"#));
            let history = harness.saved_session_history(source).await.unwrap();
            assert!(
                history.messages.iter().flat_map(|m| &m.parts).any(
                    |p| matches!(p,SavedHistoryPart::Tool {output:Some(s),..} if s=="/project")
                )
            );
            let copy = harness.copy_saved_session(source).await.unwrap();
            assert_ne!(copy.native_id, source.native_id);
            assert_eq!(
                harness
                    .saved_session_history(&copy)
                    .await
                    .unwrap()
                    .messages
                    .len(),
                history.messages.len()
            );
            fixture(
                &runtime,
                home.path(),
                "resume",
                compression,
                &copy.native_id,
            )
            .await;
            for (path, bytes) in originals {
                assert_eq!(fs::read(path).unwrap(), bytes, "source bytes changed");
            }
        }
    }
}
