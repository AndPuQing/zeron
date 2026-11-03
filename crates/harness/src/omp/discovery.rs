//! Enumerate native OMP transcripts, not the artifacts stored beside them.
use crate::HarnessError;
use std::{
    collections::HashSet,
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
    let mut visited = HashSet::new();
    for root in roots {
        let root = match root.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !visited.insert(root.clone()) {
            continue;
        }
        let mut stack = vec![(fs::read_dir(root)?, 0)];
        while let Some((entries, depth)) = stack.last_mut() {
            let Some(entry) = entries.next() else {
                stack.pop();
                continue;
            };
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() && *depth < 2 && !artifact_dir(&path) {
                let depth = *depth + 1;
                if visited.insert(path.clone()) {
                    match fs::read_dir(path) {
                        Ok(entries) => stack.push((entries, depth)),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            } else if kind.is_file()
                && path.extension().is_some_and(|e| e == "jsonl")
                && visit(path)?
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
        assert_eq!(
            found,
            vec![
                project
                    .join("session.with.dots.jsonl")
                    .canonicalize()
                    .unwrap()
            ]
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
