//! Use the pinned SDK's public stores and checkpoint reader. Copies have a
//! separate blob namespace and a marker understood by the normal resume path.
use super::CursorHarness;
use crate::{
    HarnessError,
    process::{Stdio, owned},
    saved_sessions::*,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeron_proto::HarnessId;
fn error(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}
impl CursorHarness {
    fn saved_root(&self) -> Result<PathBuf, HarnessError> {
        storage_root(
            self.saved_home.as_deref(),
            "ZERUN_CURSOR_HISTORY_DIR",
            ".cursor",
        )
    }
    pub(super) fn saved_store_id(&self) -> Result<String, HarnessError> {
        Ok(store_id(HarnessId::Cursor, &self.saved_root()?))
    }
    pub(super) async fn saved_list(
        &self,
        cursor: Option<&str>,
    ) -> Result<SavedSessionPage, HarnessError> {
        match self.saved_root() {
            Err(HarnessError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SavedSessionPage::default());
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        self.saved_call("list", cursor, None).await
    }
    pub(super) async fn saved_call<T: serde::de::DeserializeOwned>(
        &self,
        action: &str,
        cursor: Option<&str>,
        session: Option<&SavedSession>,
    ) -> Result<T, HarnessError> {
        let root = self.saved_root()?;
        let identity = self.saved_store_id()?;
        if session.is_some_and(|s| s.harness != HarnessId::Cursor || s.store_id != identity) {
            return Err(error("The Cursor history location changed. Scan again."));
        }
        let state_root = if self.saved_home.is_some() {
            root.join("zeron-state")
        } else {
            super::state::state_root()
        };
        // Rust owns the snapshots so cancellation and child timeouts clean
        // them up even when JavaScript cannot run its finally blocks.
        let scratch = tempfile::tempdir()?;
        let input=serde_json::to_vec(&json!({"action":action,"root":root,"stateRoot":state_root,"scratchRoot":scratch.path(),"identity":identity,"cursor":cursor,"session":session})).map_err(|e|error(e.to_string()))?;
        let (exe, args) = self.resolve_shim().await?;
        let mut command = crate::process::Command::new(&exe);
        command
            .args(args)
            .arg("saved")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::compose_child_path(&mut command, &exe);
        self.environment.apply(&mut command);
        owned::configure(&mut command);
        let mut child = owned::Child::new(command.spawn()?);
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(120), async {
            stdin.write_all(&input).await?;
            stdin.shutdown().await?;
            drop(stdin);
            let mut output = vec![];
            (&mut stdout)
                .take((MAX_HISTORY_BYTES + 65537) as u64)
                .read_to_end(&mut output)
                .await?;
            if output.len() > MAX_HISTORY_BYTES + 65536 {
                return Err(error("Cursor import response exceeds the limit."));
            }
            let response: serde_json::Value = serde_json::from_slice(&output)
                .map_err(|e| error(format!("Invalid Cursor import response: {e}")))?;
            if response["ev"] == "fatal" {
                return Err(error(
                    response["message"]
                        .as_str()
                        .unwrap_or("Cursor import failed."),
                ));
            }
            if !child.wait().await?.success() {
                return Err(error("Cursor history reader failed."));
            }
            serde_json::from_value(response).map_err(|e| error(e.to_string()))
        })
        .await
        .unwrap_or_else(|_| Err(error("Cursor history request timed out.")));
        child.shutdown(Duration::from_millis(100)).await;
        result
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::Harness;
    use std::{fs, path::Path};
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, rows: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, rows)
                } else {
                    rows.push((p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap()));
                }
            }
        }
        let mut rows = vec![];
        walk(dir, dir, &mut rows);
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }
    async fn fixture(module: &Path, action: &str, root: &Path, kind: &str, id: &str) {
        let output = tokio::process::Command::new("node")
            .args([
                Path::new(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/cursor_saved.mjs"
                )),
                module,
                Path::new(action),
                root,
                Path::new(kind),
                Path::new(id),
            ])
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
    #[ignore = "requires ZERON_CURSOR_TEST_MODULE pointing to pinned @cursor/sdk/dist/esm/index.js"]
    async fn native_jsonl_and_sqlite_checkpoints_copy_and_resume_independently() {
        use std::os::unix::fs::PermissionsExt;
        let module = PathBuf::from(std::env::var_os("ZERON_CURSOR_TEST_MODULE").unwrap())
            .canonicalize()
            .unwrap();
        let shim = sdk_test_shim(&module);
        let mut source = super::super::SHIM_SOURCE.to_string();
        source = source.replace(
            "import(\"@cursor/sdk\")",
            &format!(
                "import({})",
                serde_json::to_string(&reqwest::Url::from_file_path(&module).unwrap().to_string())
                    .unwrap()
            ),
        );
        source = source.replace(
            "import(\"@cursor/sdk/sqlite\")",
            &format!(
                "import({})",
                serde_json::to_string(
                    &reqwest::Url::from_file_path(module.with_file_name("sqlite.js"))
                        .unwrap()
                        .to_string()
                )
                .unwrap()
            ),
        );
        fs::write(&shim, format!("#!/usr/bin/env node\n{source}")).unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
        for kind in ["jsonl", "sqlite"] {
            let home = tempfile::tempdir().unwrap();
            fixture(&module, "create", home.path(), kind, "").await;
            let source_path = if kind == "sqlite" {
                home.path().join("projects/fixture/sdk-agent-store/fixture")
            } else {
                home.path().join("zeron-state/agents/source")
            };
            let before = snapshot(&source_path);
            let harness = CursorHarness::new()
                .with_executable(shim.to_path_buf())
                .with_saved_session_home(home.path());
            let page = harness.saved_sessions(None).await.unwrap();
            assert_eq!(page.sessions.len(), 1);
            let source = &page.sessions[0];
            assert_eq!(source.model.as_deref(), Some("fixture-model"));
            let copy = harness.copy_saved_session(source).await.unwrap();
            assert_ne!(copy.native_id, source.native_id);
            let history = harness.saved_session_history(&copy).await.unwrap();
            assert!(
                history
                    .messages
                    .iter()
                    .flat_map(|m| &m.parts)
                    .any(|p| matches!(p,SavedHistoryPart::Text(s)if s=="Remember teal"))
            );
            assert!(
                history
                    .messages
                    .iter()
                    .flat_map(|m| &m.parts)
                    .any(|p| matches!(p,SavedHistoryPart::Reasoning(s)if s=="Native reasoning"))
            );
            fixture(&module, "resume", home.path(), kind, &copy.native_id).await;
            assert_eq!(snapshot(&source_path), before, "native source changed");
            let resumed = harness.saved_session_history(&copy).await.unwrap();
            assert!(
                resumed
                    .messages
                    .iter()
                    .flat_map(|m| &m.parts)
                    .any(|p| matches!(p,SavedHistoryPart::Text(s)if s=="Independent continuation"))
            );
        }
    }
    fn sdk_test_shim(_module: &Path) -> tempfile::TempPath {
        tempfile::Builder::new()
            .suffix(".mjs")
            .tempfile()
            .unwrap()
            .into_temp_path()
    }
}
