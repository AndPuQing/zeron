//! Enumerate native OMP transcripts, not the artifacts stored beside them.
use crate::HarnessError;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

/// OMP stores `<session>.jsonl` beside `<session>/` containing tool output and
/// child transcripts. Those files are not independent importable sessions.
fn artifact_dir(path: &Path) -> bool {
    let mut sibling = path.as_os_str().to_os_string();
    sibling.push(".jsonl");
    // Even a symlinked transcript identifies an artifact directory; neither
    // it nor the files beneath it should become a discovery candidate.
    fs::symlink_metadata(PathBuf::from(sibling)).is_ok_and(|m| !m.is_dir())
}

/// Visit transcript paths lazily; return true to stop after finding a session.
/// No global entry-count limit: an unrelated log or a large legitimate store
/// must not make all history unavailable. Depth and open directory handles
/// remain bounded. Child symlinks are never followed; explicitly configured
/// roots may be symlinked and are deduplicated by their canonical identity.
pub(super) fn visit_files(
    roots: &[PathBuf],
    mut visit: impl FnMut(PathBuf) -> Result<bool, HarnessError>,
) -> Result<(), HarnessError> {
    walk(roots, IoPolicy::Report, false, |path, _| visit(path))
}

/// Resume is a best-effort search: an unreadable directory must not prevent
/// finding the requested identity in another directory or configured root.
/// Listing remains strict so a partial inventory is never presented as complete.
/// Resume also descends into artifact directories, flagging their files, so a
/// child transcript copy written beside its source stays reachable by name.
pub(super) fn visit_files_for_resume(
    roots: &[PathBuf],
    visit: impl FnMut(PathBuf, bool) -> Result<bool, HarnessError>,
) -> Result<(), HarnessError> {
    walk(roots, IoPolicy::Skip, true, visit)
}

#[derive(Clone, Copy)]
enum IoPolicy {
    Report,
    Skip,
}

impl IoPolicy {
    fn read<T>(self, result: std::io::Result<T>) -> Result<Option<T>, HarnessError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(e) if matches!(self, Self::Skip) || e.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn open_dir(
    path: &Path,
    depth: usize,
    visited: &mut HashMap<PathBuf, usize>,
    policy: IoPolicy,
) -> Result<Option<fs::ReadDir>, HarnessError> {
    // A directory reached from an enclosing root has less depth left. A later
    // explicit root must be allowed to revisit it with its full depth budget.
    if visited.get(path).is_some_and(|previous| *previous <= depth) {
        return Ok(None);
    }
    let entries = policy.read(fs::read_dir(path))?;
    if entries.is_some() {
        visited.insert(path.to_owned(), depth);
    }
    Ok(entries)
}

fn walk(
    roots: &[PathBuf],
    policy: IoPolicy,
    artifacts: bool,
    mut visit: impl FnMut(PathBuf, bool) -> Result<bool, HarnessError>,
) -> Result<(), HarnessError> {
    let mut visited = HashMap::new();
    let mut files = HashSet::new();
    for root in roots {
        let Some(root) = policy.read(root.canonicalize())? else {
            continue;
        };
        let Some(entries) = open_dir(&root, 0, &mut visited, policy)? else {
            continue;
        };
        let mut stack = vec![(entries, 0, false)];
        while let Some((entries, depth, inside)) = stack.last_mut() {
            let Some(entry) = entries.next() else {
                stack.pop();
                continue;
            };
            let Some(entry) = policy.read(entry)? else {
                continue;
            };
            let Some(kind) = policy.read(entry.file_type())? else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() && *depth < 2 {
                let artifact = *inside || artifact_dir(&path);
                if artifact && !artifacts {
                    continue;
                }
                let depth = *depth + 1;
                if let Some(entries) = open_dir(&path, depth, &mut visited, policy)? {
                    stack.push((entries, depth, artifact));
                }
            } else if kind.is_file()
                && path.extension().is_some_and(|e| e == "jsonl")
                && files.insert(path.clone())
                && visit(path, *inside)?
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_roots_keep_each_roots_depth_budget_without_duplicate_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        let custom = root.join("custom");
        fs::create_dir_all(custom.join("project/legacy/too-deep")).unwrap();
        let shallow = custom.join("project/session.jsonl");
        let deep = custom.join("project/legacy/session.jsonl");
        fs::write(&shallow, "").unwrap();
        fs::write(&deep, "").unwrap();
        fs::write(custom.join("project/legacy/too-deep/session.jsonl"), "").unwrap();
        let expected = HashSet::from([
            shallow.canonicalize().unwrap(),
            deep.canonicalize().unwrap(),
        ]);
        for roots in [vec![root.clone(), custom.clone()], vec![custom, root]] {
            let mut found = Vec::new();
            visit_files(&roots, |path| {
                found.push(path);
                Ok(false)
            })
            .unwrap();
            assert_eq!(
                found.len(),
                2,
                "overlapping roots must not duplicate callbacks"
            );
            assert_eq!(found.into_iter().collect::<HashSet<_>>(), expected);
        }
    }

    #[test]
    fn resume_skips_invalid_roots_but_listing_reports_them_and_callbacks_still_fail() {
        let dir = tempfile::tempdir().unwrap();
        let invalid = dir.path().join("not-a-directory");
        fs::write(&invalid, "").unwrap();
        let good = dir.path().join("sessions");
        fs::create_dir(&good).unwrap();
        let transcript = good.join("session.jsonl");
        fs::write(&transcript, "").unwrap();
        let roots = [invalid, good];
        assert!(visit_files(&roots, |_| Ok(false)).is_err());
        let mut found = vec![];
        visit_files_for_resume(&roots, |path, _| {
            found.push(path);
            Ok(true)
        })
        .unwrap();
        assert_eq!(found, [transcript.canonicalize().unwrap()]);
        let error = visit_files_for_resume(&roots, |_, _| {
            Err(HarnessError::Protocol("visitor failure".into()))
        })
        .unwrap_err();
        assert!(error.to_string().contains("visitor failure"));
    }

    #[cfg(unix)]
    #[test]
    fn resume_skips_permission_denied_roots_and_children_without_hiding_listing_errors() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let denied = dir.path().join("denied");
        let good = dir.path().join("sessions");
        let child = good.join("unreadable-project");
        fs::create_dir_all(&denied).unwrap();
        fs::create_dir_all(&child).unwrap();
        let transcript = good.join("session.jsonl");
        fs::write(&transcript, "").unwrap();
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let _root_guard = Restore(denied.clone());
        let _child_guard = Restore(child.clone());
        fs::set_permissions(&denied, fs::Permissions::from_mode(0)).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0)).unwrap();
        // Root/privileged test runners can bypass mode bits.
        if fs::read_dir(&denied).is_ok() {
            return;
        }
        for roots in [vec![denied.clone(), good.clone()], vec![good, denied]] {
            assert!(visit_files(&roots, |_| Ok(false)).is_err());
            let mut found = vec![];
            visit_files_for_resume(&roots, |path, _| {
                found.push(path);
                Ok(false)
            })
            .unwrap();
            assert_eq!(found, [transcript.canonicalize().unwrap()]);
        }
    }

    #[test]
    fn more_than_twenty_thousand_transcripts_are_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..20_001 {
            fs::write(dir.path().join(format!("session-{index}.jsonl")), "").unwrap();
        }
        let mut count = 0;
        visit_files(&[dir.path().to_owned()], |_| {
            count += 1;
            Ok(false)
        })
        .unwrap();
        assert_eq!(count, 20_001);
    }

    #[test]
    fn skips_artifacts_even_when_they_contain_transcript_shaped_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let project = root.join("project");
        // Dots in session basenames must be preserved when finding siblings.
        let artifacts = project.join("session.with.dots");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(project.join("session.with.dots.jsonl"), "parent").unwrap();
        fs::write(artifacts.join("child.jsonl"), "not a top-level session").unwrap();
        let mut found = vec![];
        visit_files(&[root.into(), project.clone()], |p| {
            found.push(p);
            Ok(false)
        })
        .unwrap();
        let parent = project
            .join("session.with.dots.jsonl")
            .canonicalize()
            .unwrap();
        assert_eq!(found, vec![parent.clone()]);
        // Resume may look inside artifacts, but must know which files they are.
        let mut found = HashSet::new();
        visit_files_for_resume(&[root.into()], |p, artifact| {
            found.insert((p, artifact));
            Ok(false)
        })
        .unwrap();
        assert_eq!(
            found,
            HashSet::from([
                (parent, false),
                (artifacts.join("child.jsonl").canonicalize().unwrap(), true),
            ])
        );
    }

    #[test]
    fn keeps_depth_bound_and_supports_flat_custom_roots() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        fs::write(dir.path().join("flat.jsonl"), "").unwrap();
        fs::write(dir.path().join("a/b/legacy.jsonl"), "").unwrap();
        fs::write(dir.path().join("a/b/c/too-deep.jsonl"), "").unwrap();
        let mut found = vec![];
        visit_files(&[dir.path().into()], |p| {
            found.push(p);
            Ok(false)
        })
        .unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|p| p.ends_with("flat.jsonl")));
        assert!(found.iter().any(|p| p.ends_with("legacy.jsonl")));
        let mut calls = 0;
        visit_files(&[dir.path().into()], |_| {
            calls += 1;
            Ok(true)
        })
        .unwrap();
        assert_eq!(calls, 1, "resume can stop without scanning the whole store");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_child_symlinks_but_deduplicates_configured_root_aliases() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("real.jsonl"), "").unwrap();
        fs::write(outside.join("other.jsonl"), "").unwrap();
        symlink(&outside, root.join("linked-project")).unwrap();
        symlink(outside.join("other.jsonl"), root.join("linked.jsonl")).unwrap();
        let alias = dir.path().join("alias");
        symlink(&root, &alias).unwrap();
        let mut found = vec![];
        visit_files(&[root.clone(), alias, dir.path().join("missing")], |p| {
            found.push(p);
            Ok(false)
        })
        .unwrap();
        assert_eq!(found, vec![root.canonicalize().unwrap().join("real.jsonl")]);
    }
}
