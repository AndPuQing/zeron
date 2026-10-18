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
        let input = serde_json::to_vec(&json!({"action": action, "home": home, "root": self.dsh_saved_root()?, "profile": profile, "identity": identity, "cursor": cursor, "session": session})).map_err(|e| error(e.to_string()))?;
        let node = crate::executable::find_on_paths("node", vec![])
            .ok_or_else(|| error("Node.js is required to read dsh's native history."))?;
        helper(&node, &input, &self.environment).await
    }
}
async fn helper<T: serde::de::DeserializeOwned>(
    node: &Path,
    input: &[u8],
    environment: &crate::environment::EnvironmentSnapshot,
) -> Result<T, HarnessError> {
    let mut command = Command::new(node);
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
    async fn fixture(home: &Path, action: &str, compression: &str, copy: &str) {
        let script = home.join("fixture.mjs");
        fs::write(&script, include_str!("../../tests/fixtures/dsh_saved.mjs")).unwrap();
        let output = tokio::process::Command::new("node")
            .arg(script)
            .arg(home)
            .arg(action)
            .arg(compression)
            .arg(copy)
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
    #[tokio::test]
    #[ignore = "requires installed dsh packages: ZERON_DSH_TEST_PACKAGES=<profiles directory>"]
    async fn native_plain_and_compressed_copies_preserve_source_tools_and_resume_independently() {
        let packages =
            std::env::var_os("ZERON_DSH_TEST_PACKAGES").expect("set ZERON_DSH_TEST_PACKAGES");
        for compression in ["none", "zstd"] {
            let home = tempfile::tempdir().unwrap();
            fs::create_dir(home.path().join("sessions")).unwrap();
            std::os::unix::fs::symlink(&packages, home.path().join("profiles")).unwrap();
            fixture(home.path(), "create", compression, "").await;
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
            fixture(home.path(), "resume", compression, &copy.native_id).await;
            for (path, bytes) in originals {
                assert_eq!(fs::read(path).unwrap(), bytes, "source bytes changed");
            }
        }
    }
}
