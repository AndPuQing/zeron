//! Private, acknowledged and revision-checked device-local configuration.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};
use zeron_harness::environment::EnvironmentSnapshot;
use zeron_proto::{
    HarnessEnvironmentMetadata, HarnessId, PatchHarnessEnvironmentParams,
    PatchHarnessEnvironmentReply, RevealHarnessEnvironmentParams, RevealedEnvironmentValue,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    schema_version: u32,
    providers: HashMap<HarnessId, EnvironmentSnapshot>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            schema_version: 1,
            providers: HashMap::new(),
        }
    }
}
struct State {
    path: Option<PathBuf>,
    configuration: Result<Configuration, String>,
}
pub struct EnvironmentStore {
    state: Mutex<State>,
    changes: tokio::sync::watch::Sender<u64>,
}
impl Default for EnvironmentStore {
    fn default() -> Self {
        Self {
            changes: tokio::sync::watch::channel(0).0,
            state: Mutex::new(State {
                path: None,
                configuration: Ok(Configuration::default()),
            }),
        }
    }
}

impl EnvironmentStore {
    pub fn subscribe_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }
    pub fn load(&self, data_dir: &Path) {
        let path = data_dir.join("agent-environment.json");
        let configuration = read(&path);
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = State {
            path: Some(path),
            configuration,
        };
    }

    pub fn snapshot(&self, harness: HarnessId) -> Result<Arc<EnvironmentSnapshot>, String> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let configuration = state.configuration.as_ref().map_err(Clone::clone)?;
        Ok(Arc::new(
            configuration
                .providers
                .get(&harness)
                .cloned()
                .unwrap_or_default(),
        ))
    }

    pub fn metadata(&self, harness: HarnessId) -> Result<HarnessEnvironmentMetadata, String> {
        Ok(self.snapshot(harness)?.metadata())
    }

    /// Order ordinary mailbox acceptance against a concurrent committed save.
    pub(crate) fn while_current<T>(
        &self,
        harness: HarnessId,
        revision: &str,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let configuration = state.configuration.as_ref().ok()?;
        let current = configuration
            .providers
            .get(&harness)
            .map_or(zeron_harness::environment::INITIAL_REVISION, |snapshot| {
                snapshot.revision.as_str()
            });
        (current == revision).then(action)
    }

    pub fn patch(
        &self,
        params: PatchHarnessEnvironmentParams,
    ) -> Result<PatchHarnessEnvironmentReply, String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let configuration = state.configuration.as_ref().map_err(Clone::clone)?;
        let previous = configuration
            .providers
            .get(&params.harness)
            .cloned()
            .unwrap_or_default();
        if previous.revision != params.expected_revision {
            return Ok(PatchHarnessEnvironmentReply {
                conflict: true,
                metadata: previous.metadata(),
            });
        }
        let mut changed = previous.patched(&params.changes)?;
        if previous.entries == changed.entries {
            return Ok(PatchHarnessEnvironmentReply {
                conflict: false,
                metadata: previous.metadata(),
            });
        }
        changed.revision = uuid::Uuid::new_v4().to_string();
        let mut candidate = configuration.clone();
        candidate.providers.insert(params.harness, changed.clone());
        if let Some(path) = &state.path {
            let bytes = serde_json::to_vec_pretty(&candidate)
                .map_err(|_| "Could not encode environment settings")?;
            write_private_atomic(path, &bytes)
                .map_err(|error| format!("Could not save environment settings: {error}"))?;
        }
        // Publish only after the complete file has been committed. Readers and
        // competing editors share this lock; a failed write changes nothing.
        state.configuration = Ok(candidate);
        self.changes
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        Ok(PatchHarnessEnvironmentReply {
            conflict: false,
            metadata: changed.metadata(),
        })
    }

    pub fn reveal(
        &self,
        params: RevealHarnessEnvironmentParams,
    ) -> Result<RevealedEnvironmentValue, String> {
        let snapshot = self.snapshot(params.harness)?;
        if snapshot.revision != params.expected_revision {
            return Err("Environment configuration changed; reload before showing a value".into());
        }
        let entry = snapshot.entries.iter().find(|(name, _)| {
            zeron_harness::environment::normalized_name(name)
                == zeron_harness::environment::normalized_name(&params.name)
        });
        match entry {
            Some((_, zeron_harness::environment::EnvironmentEntry::Set { value, .. })) => {
                Ok(RevealedEnvironmentValue {
                    revision: snapshot.revision.clone(),
                    value: value.clone(),
                })
            }
            _ => Err("This environment variable has no configured value".into()),
        }
    }
}

fn read(path: &Path) -> Result<Configuration, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Configuration::default()),
        Err(_) => return Err("Could not read agent-environment.json; provider launches are blocked until the configuration is repaired".into()),
    };
    if bytes.len() > 1024 * 1024 {
        return Err("agent-environment.json exceeds its size limit".into());
    }
    let configuration: Configuration = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid agent-environment.json; the original file has been preserved")?;
    if configuration.schema_version != 1 {
        return Err(
            "Unsupported agent-environment.json schema; the original file has been preserved"
                .into(),
        );
    }
    for snapshot in configuration.providers.values() {
        snapshot.validate()?;
    }
    Ok(configuration)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing configuration directory"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".agent-environment.")
        .suffix(".tmp")
        .tempfile_in(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC, WRITE_OWNER};
        // tempfile's data handle lacks the security update rights.
        // SetSecurityInfo also reads the existing descriptor while processing
        // file DACL inheritance, so include READ_CONTROL in this handle.
        // Protect the owner and DACL before writing any configuration bytes.
        let security_handle = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(READ_CONTROL | WRITE_DAC | WRITE_OWNER)
            .open(temporary.path())
            .map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("Opening private file security handle: {error}"),
                )
            })?;
        private_windows_file(&security_handle)?;
    }
    temporary.write_all(bytes).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("Writing private configuration: {error}"),
        )
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("Syncing private configuration: {error}"),
        )
    })?;
    temporary.persist(path).map_err(|error| {
        std::io::Error::new(
            error.error.kind(),
            format!("Replacing private configuration: {}", error.error),
        )
    })?;
    // A directory sync failure after rename must not report a failed save:
    // the file is already committed and in-memory state must match it.
    #[cfg(unix)]
    {
        let _ = std::fs::File::open(directory).and_then(|file| file.sync_all());
    }
    Ok(())
}

#[cfg(windows)]
fn current_windows_user() -> std::io::Result<Vec<usize>> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: Windows returns an owned token handle. The query writes into an
    // aligned, sufficiently large allocation that owns the returned SID bytes.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut size,
        );
        if (size as usize) < std::mem::size_of::<TOKEN_USER>() {
            return Err(std::io::Error::last_os_error());
        }
        let mut user = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            user.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(user)
    }
}

#[cfg(windows)]
fn private_windows_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT,
                SetSecurityInfo,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, OWNER_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_USER,
        },
    };
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)\0".encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    let user = current_windows_user()?;
    // SAFETY: Windows allocates the descriptor; the live file handle is owned
    // by the caller. The protected DACL grants access only to its owner.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = std::ptr::null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0 {
            let error = std::io::Error::last_os_error();
            LocalFree(descriptor);
            return Err(error);
        }
        let result = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION
                | OWNER_SECURITY_INFORMATION,
            // Elevated Windows processes may otherwise create files owned by
            // the Administrators group. OWNER_RIGHTS must refer to this user.
            (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid,
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        );
        let error = (result != 0).then(|| std::io::Error::from_raw_os_error(result as i32));
        LocalFree(descriptor);
        if let Some(error) = error {
            return Err(std::io::Error::new(
                error.kind(),
                format!("Protecting private file owner and ACL: {error}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeron_proto::EnvironmentChange;
    fn patch(revision: &str, value: &str) -> PatchHarnessEnvironmentParams {
        PatchHarnessEnvironmentParams {
            harness: HarnessId::Grok,
            expected_revision: revision.into(),
            changes: vec![EnvironmentChange::Set {
                name: "API_KEY".into(),
                value: value.into(),
            }],
            target_device_id: None,
        }
    }
    #[test]
    fn persistence_conflicts_noops_and_device_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let store = EnvironmentStore::default();
        store.load(dir.path());
        let initial = store.metadata(HarnessId::Grok).unwrap();
        let saved = store
            .patch(patch(&initial.revision, "private-value"))
            .unwrap();
        assert!(!saved.conflict);
        assert_ne!(saved.metadata.revision, initial.revision);
        assert!(
            store
                .patch(patch(&initial.revision, "stale"))
                .unwrap()
                .conflict
        );
        assert_eq!(
            store
                .patch(patch(&saved.metadata.revision, "private-value"))
                .unwrap()
                .metadata,
            saved.metadata
        );
        assert!(store.snapshot(HarnessId::Codex).unwrap().entries.is_empty());
        assert!(
            EnvironmentStore::default()
                .snapshot(HarnessId::Grok)
                .unwrap()
                .entries
                .is_empty()
        );
        let restarted = EnvironmentStore::default();
        restarted.load(dir.path());
        assert_eq!(restarted.metadata(HarnessId::Grok).unwrap(), saved.metadata);
        let reveal = RevealHarnessEnvironmentParams {
            harness: HarnessId::Grok,
            name: "API_KEY".into(),
            expected_revision: saved.metadata.revision.clone(),
            target_device_id: None,
        };
        assert_eq!(restarted.reveal(reveal).unwrap().value, "private-value");
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("private-value")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join("agent-environment.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn failed_write_and_invalid_files_preserve_committed_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = EnvironmentStore::default();
        store.load(dir.path());
        let initial = store.metadata(HarnessId::Grok).unwrap();
        std::fs::create_dir(dir.path().join("agent-environment.json")).unwrap();
        assert!(store.patch(patch(&initial.revision, "secret")).is_err());
        assert_eq!(store.metadata(HarnessId::Grok).unwrap(), initial);
        let other = tempfile::tempdir().unwrap();
        let path = other.path().join("agent-environment.json");
        for bytes in [
            "not json with private-value",
            "{\"schemaVersion\":99,\"providers\":{}}",
        ] {
            std::fs::write(&path, bytes).unwrap();
            store.load(other.path());
            let error = store.snapshot(HarnessId::Grok).unwrap_err();
            assert!(!error.contains("private-value"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn concurrent_editors_have_one_winner_and_clearing_retains_revision() {
        let store = Arc::new(EnvironmentStore::default());
        let initial = store.metadata(HarnessId::Grok).unwrap().revision;
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let store = store.clone();
                let revision = initial.clone();
                std::thread::spawn(move || store.patch(patch(&revision, &i.to_string())).unwrap())
            })
            .collect();
        let replies: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(replies.iter().filter(|reply| !reply.conflict).count(), 1);
        let revision = store.metadata(HarnessId::Grok).unwrap().revision;
        let reply = store
            .patch(PatchHarnessEnvironmentParams {
                changes: vec![EnvironmentChange::Delete {
                    name: "API_KEY".into(),
                }],
                ..patch(&revision, "ignored")
            })
            .unwrap();
        assert!(reply.metadata.entries.is_empty());
        assert_ne!(reply.metadata.revision, initial);
        assert!(store.patch(patch(&revision, "stale")).unwrap().conflict);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo,
                SE_FILE_OBJECT,
            },
            DACL_SECURITY_INFORMATION, EqualSid, OWNER_SECURITY_INFORMATION, TOKEN_USER,
        },
    };
    #[test]
    fn atomic_replacements_keep_a_protected_owner_only_acl() {
        let root = tempfile::tempdir().unwrap();
        let store = EnvironmentStore::default();
        store.load(root.path());
        for value in ["first-secret", "replacement-secret"] {
            let revision = store.metadata(HarnessId::Grok).unwrap().revision;
            store
                .patch(PatchHarnessEnvironmentParams {
                    harness: HarnessId::Grok,
                    expected_revision: revision,
                    target_device_id: None,
                    changes: vec![zeron_proto::EnvironmentChange::Set {
                        name: "API_KEY".into(),
                        value: value.into(),
                    }],
                })
                .unwrap();
            let file = std::fs::File::open(root.path().join("agent-environment.json")).unwrap();
            let mut descriptor = std::ptr::null_mut();
            let mut sddl = std::ptr::null_mut();
            let mut length = 0;
            let mut owner = std::ptr::null_mut();
            let user = current_windows_user().unwrap();
            // SAFETY: GetSecurityInfo allocates a live descriptor, converted
            // to another Windows allocation; both are freed after inspection.
            unsafe {
                assert_eq!(
                    GetSecurityInfo(
                        file.as_raw_handle(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                        &mut owner,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut descriptor
                    ),
                    0
                );
                assert_ne!(
                    EqualSid(owner, (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid),
                    0,
                    "the file owner must be the current user, including elevated processes"
                );
                let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut sddl,
                    &mut length,
                );
                if converted == 0 {
                    LocalFree(descriptor);
                    panic!("ACL conversion failed: {}", std::io::Error::last_os_error());
                }
                let text =
                    String::from_utf16_lossy(std::slice::from_raw_parts(sddl, length as usize));
                LocalFree(sddl.cast());
                LocalFree(descriptor);
                assert!(
                    text.starts_with("D:P"),
                    "DACL must not inherit grants: {text}"
                );
                assert_eq!(text.matches('(').count(), 1, "only one owner grant: {text}");
                assert!(
                    text.contains(";;;OW)"),
                    "grant must target owner rights: {text}"
                );
            }
        }
    }
}
