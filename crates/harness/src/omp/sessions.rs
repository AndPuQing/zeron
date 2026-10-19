//! Host-local UUID -> native OMP file lookup. Never rewrite OMP's own files.
//!
//! The index is rooted at `<agent>/zeron-omp-sessions`, a dedicated namespace
//! beside Pi's `<agent>/zeron-sessions`. Both products honor
//! `PI_CODING_AGENT_DIR`, so a user can point them at one directory; the
//! distinct namespace plus the OMP-format marker in [`omp_marker`] keep an OMP
//! chat from resolving (or importing) a Pi conversation and the reverse.
use crate::HarnessError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct Store {
    root: PathBuf,
    agent: PathBuf,
}
/// OMP's settings directory: an explicit override, else `PI_CODING_AGENT_DIR`
/// (OMP keeps Pi's env prefix), else `~/.omp/agent`.
pub(super) fn agent_dir(explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| {
        crate::model_context::root(
            "PI_CODING_AGENT_DIR",
            crate::executable::home_or_current_dir().join(".omp/agent"),
        )
    })
}
impl Store {
    pub fn new(root: Option<PathBuf>, agent: Option<PathBuf>) -> Self {
        let agent = agent_dir(agent);
        Self {
            // A DEDICATED namespace: Pi's index is `<agent>/zeron-sessions`, so
            // an explicit shared `PI_CODING_AGENT_DIR` would otherwise make the
            // two harnesses read and write the same UUID -> file records.
            root: root.unwrap_or_else(|| agent.join("zeron-omp-sessions")),
            agent,
        }
    }
    fn key(&self, id: &str) -> PathBuf {
        self.root
            .join(format!("{:x}.json", Sha256::digest(id.as_bytes())))
    }
    pub fn remember(&self, id: &str, file: &Path) -> Result<(), HarnessError> {
        if !file.is_absolute() {
            return Err(HarnessError::Protocol(
                "OMP returned a non-absolute session file".into(),
            ));
        }
        self.write(id, &json!({"sessionId":id,"sessionFile":file}))
    }
    fn write(&self, id: &str, value: &Value) -> Result<(), HarnessError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.key(id);
        if json_file(&path) == *value {
            return Ok(());
        }
        let temp = self.root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temp, serde_json::to_vec(value).unwrap())?;
        let result = std::fs::rename(&temp, &path);
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result.map_err(Into::into)
    }
    /// OMP resumes a session by absolute file path (`--resume <path>`); its own
    /// resolver also accepts an id prefix, but pinning the verified file keeps a
    /// moved or renamed store from silently opening a different conversation.
    pub fn resume_args(&self, id: &str, cwd: &Path) -> Result<Vec<String>, HarnessError> {
        let file = self.resolve(id, cwd)?;
        Ok(vec!["--resume".into(), file.display().to_string()])
    }
    pub fn resolve(&self, id: &str, _cwd: &Path) -> Result<PathBuf, HarnessError> {
        let mut candidates = vec![];
        if let Some(file) = json_file(&self.key(id))["sessionFile"].as_str() {
            candidates.push(PathBuf::from(file));
        }
        for file in candidates {
            if matches_id(&file, id) {
                return Ok(file.canonicalize()?);
            }
        }
        for root in self.roots() {
            // OMP stores session files under per-project directories at depth 2.
            // Symlinked directories are not followed, so an escaped link cannot
            // make an unrelated file look like this session.
            let mut dirs = vec![(root, 0)];
            let mut seen = 0;
            while let Some((dir, depth)) = dirs.pop() {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    seen += 1;
                    if seen > 20000 {
                        return Err(HarnessError::Protocol(
                            "OMP session search exceeded 20000 entries".into(),
                        ));
                    }
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    let path = entry.path();
                    if kind.is_dir() && depth < 2 {
                        dirs.push((path, depth + 1));
                    } else if kind.is_file()
                        && path.extension().is_some_and(|e| e == "jsonl")
                        && matches_id(&path, id)
                    {
                        return Ok(path.canonicalize()?);
                    }
                }
            }
        }
        Err(HarnessError::Protocol(format!(
            "native OMP session file for {id} was not found"
        )))
    }
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.agent.join("sessions")];
        if let Some(dir) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
            roots.push(PathBuf::from(dir));
        }
        roots
    }
}
fn json_file(path: &Path) -> Value {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}
/// OMP-only markers in a session's head. Current OMP writes a fixed-width
/// `title` slot, a per-entry `runtime` identity, the `model_change.model`
/// spelling and `thinking_level_change.configured`; Pi v2/v3 writes
/// `modelId`/`provider` and none of the others. This is what keeps a Pi file
/// from being accepted as an OMP conversation when the user deliberately
/// points both products at one native directory.
pub(super) fn omp_marker(records: &[Value]) -> bool {
    records.iter().any(|r| {
        matches!(r["type"].as_str(), Some("title" | "title_change"))
            || r["runtime"].is_object()
            || (r["type"] == "model_change"
                && r.get("model").is_some()
                && r.get("modelId").is_none())
            || (r["type"] == "thinking_level_change" && r.get("configured").is_some())
    })
}
/// OMP's first line is a `title` slot, so the `session` header is not
/// necessarily line 0; scan the bounded head for it rather than assuming a
/// position, and require an OMP marker so Pi files are never mistaken for OMP.
fn matches_id(path: &Path, id: &str) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file.take(262144));
    let mut records = vec![];
    let mut budget = 512;
    while budget > 0 {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        budget -= 1;
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            records.push(value);
        }
    }
    records
        .iter()
        .any(|r| r["type"] == "session" && r["id"] == id)
        && omp_marker(&records)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn omp_session(id: &str, cwd: &Path) -> PathBuf {
        let dir = cwd.join("sessions/project");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("2026-01-01T00-00-00-000Z_{id}.jsonl"));
        let mut file = std::fs::File::create(&path).unwrap();
        use std::io::Write;
        writeln!(
            file,
            "{}",
            json!({"type":"title","v":1,"title":"t","updatedAt":"2026-01-01T00:00:00Z","pad":""})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session","version":3,"id":id,"timestamp":"2026-01-01T00:00:00Z","cwd":cwd})
        )
        .unwrap();
        path
    }
    #[test]
    fn finds_omp_sessions_behind_the_title_slot_and_rejects_wrong_identity() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        omp_session("omp-id", &agent);
        let store = Store::new(Some(dir.path().join("index")), Some(agent.clone()));
        let found = store.resolve("omp-id", dir.path()).unwrap();
        assert!(found.ends_with("2026-01-01T00-00-00-000Z_omp-id.jsonl"));
        assert!(store.resolve("other-id", dir.path()).is_err());
    }
    #[test]
    fn a_shared_pi_agent_dir_still_uses_a_separate_index_and_never_resolves_pi_files() {
        fn pi_session(id: &str, cwd: &Path) -> PathBuf {
            let dir = cwd.join("sessions/pi-project");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("2026-01-01T00-00-00-000Z_{id}.jsonl"));
            let mut file = std::fs::File::create(&path).unwrap();
            use std::io::Write;
            writeln!(
                file,
                "{}",
                json!({"type":"session","version":3,"id":id,"timestamp":"2026-01-01T00:00:00Z","cwd":cwd})
            )
            .unwrap();
            writeln!(
                file,
                "{}",
                json!({"type":"model_change","id":"m","parentId":null,"provider":"p","modelId":"m"})
            )
            .unwrap();
            path
        }
        let dir = tempfile::tempdir().unwrap();
        // The same native directory (a user-set shared PI_CODING_AGENT_DIR),
        // even the same UUID spelled in both formats.
        let agent = dir.path().join("agent");
        let omp = omp_session("shared-id", &agent);
        pi_session("shared-id", &agent);
        pi_session("pi-only", &agent);
        let store = Store::new(None, Some(agent.clone()));
        assert_eq!(store.root, agent.join("zeron-omp-sessions"));
        assert_ne!(store.root, agent.join("zeron-sessions"));
        assert_eq!(
            store.resolve("shared-id", dir.path()).unwrap(),
            omp.canonicalize().unwrap()
        );
        assert!(
            store.resolve("pi-only", dir.path()).is_err(),
            "a Pi-format file must never resolve as an OMP session"
        );
    }
    #[test]
    fn remember_round_trips_and_missing_file_is_not_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent");
        let file = omp_session("abcd-1234", &agent);
        let store = Store::new(Some(dir.path().join("index")), Some(agent));
        store.remember("abcd-1234", &file).unwrap();
        assert_eq!(
            store.resolve("abcd-1234", dir.path()).unwrap(),
            file.canonicalize().unwrap()
        );
        std::fs::remove_file(&file).unwrap();
        assert!(store.resolve("abcd-1234", dir.path()).is_err());
    }
}
