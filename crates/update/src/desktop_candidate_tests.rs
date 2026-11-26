use super::*;
use std::process::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Exercise the updater against the packages produced by the release jobs.
#[tokio::test]
#[ignore = "requires the desktop release workflow's packages"]
async fn desktop_candidate_upgrades() {
    let candidate = PathBuf::from(std::env::var_os("ZERUN_CANDIDATE_PACKAGE").unwrap());
    let version = current_version();
    let file = candidate.file_name().unwrap().to_str().unwrap().to_owned();
    let manifest = Manifest {
        version: version.into(),
        files: [(
            file.clone(),
            FileMeta {
                sha256: Some(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(&candidate).unwrap())
                )),
            },
        )]
        .into(),
        ..Default::default()
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let metadata = serde_json::to_vec(&manifest).unwrap();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = [0; 4096];
            let n = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            if request.starts_with(&format!("GET /releases/{file} ")) {
                let mut archive = tokio::fs::File::open(&candidate).await.unwrap();
                let size = archive.metadata().await.unwrap().len();
                socket.write_all(response(size).as_bytes()).await.unwrap();
                tokio::io::copy(&mut archive, &mut socket).await.unwrap();
            } else if request.starts_with("GET /releases/manifest") {
                socket
                    .write_all(response(metadata.len() as u64).as_bytes())
                    .await
                    .unwrap();
                socket.write_all(&metadata).await.unwrap();
            } else {
                panic!("unexpected release request: {request}");
            }
        }
    });

    for (key, previous) in [
        ("ZERUN_REFERENCE_GPUI", "0.3.30"),
        ("ZERUN_REFERENCE_BETA", "0.3.31-beta.1"),
    ] {
        let archive = PathBuf::from(std::env::var_os(key).unwrap());
        let root = tempfile::tempdir().unwrap();
        let reference = root.path().join("reference");
        std::fs::create_dir(&reference).unwrap();
        assert!(
            Command::new("tar")
                .arg("-xf")
                .arg(archive)
                .arg("-C")
                .arg(&reference)
                .status()
                .unwrap()
                .success()
        );
        let profile = root.path().join("profile");
        std::fs::create_dir(&profile).unwrap();
        let saved = profile.join("saved-chat");
        std::fs::write(&saved, b"existing conversation").unwrap();
        install(&base, &manifest, &reference, &profile, previous).await;
        assert_eq!(std::fs::read(saved).unwrap(), b"existing conversation");
        println!("{previous} -> {version} verified");
    }
    server.abort();
}

fn response(length: u64) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n")
}

#[cfg(target_os = "linux")]
async fn install(base: &str, manifest: &Manifest, reference: &Path, _: &Path, previous: &str) {
    let app_root = reference.parent().unwrap().join("app");
    std::fs::create_dir(&app_root).unwrap();
    let package = std::fs::read_dir(reference)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let installed = app_root.join(managed_dir(previous));
    std::fs::rename(package, &installed).unwrap();
    apply_headless(&app_root, previous).unwrap();
    assert_version(&app_root.join("current/zerun"), "zerun", previous);
    stage_headless(base, manifest, &app_root).await.unwrap();
    apply_headless(&app_root, &manifest.version).unwrap();
    let current = app_root.join("current");
    assert_version(&current.join("zerun"), "zerun", &manifest.version);
    assert_version(
        &current.join("zerun-engine"),
        "zerun-engine",
        &manifest.version,
    );
    assert!(current.join("ui/app.luau").is_file());
    assert!(current.join("plugins-builtin").is_dir());
}

#[cfg(target_os = "macos")]
async fn install(
    base: &str,
    manifest: &Manifest,
    reference: &Path,
    profile: &Path,
    previous: &str,
) {
    let bundle = reference.join("Zerun.app");
    assert_eq!(bundle_app_version(&bundle).as_deref(), Some(previous));
    let staged = stage_mac_app(base, manifest, profile).await.unwrap();
    apply_mac_app(&staged, &bundle).unwrap();
    assert_eq!(
        bundle_app_version(&bundle).as_deref(),
        Some(manifest.version.as_str())
    );
    assert_version(
        &bundle.join("Contents/MacOS/Zerun Engine"),
        "zerun-engine",
        &manifest.version,
    );
    assert!(bundle.join("Contents/Resources/ui/app.luau").is_file());
    assert!(bundle.join("Contents/Resources/plugins-builtin").is_dir());
    assert!(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle)
            .status()
            .unwrap()
            .success()
    );
}

#[cfg(windows)]
async fn install(base: &str, manifest: &Manifest, reference: &Path, _: &Path, previous: &str) {
    let exe = reference.join("zerun.exe");
    let version = &manifest.version;
    assert_version(&exe, "zerun", previous);
    if Channel::of_version(previous) == Channel::Beta {
        windows::set_packaged();
        let staged = windows::stage(base, manifest, reference).await.unwrap();
        // As when the engine trials a staged desktop before committing it.
        windows::install_version_files(&staged, reference).unwrap();
        windows::commit_trial(&staged, reference, version).unwrap();
    } else {
        let standalone = std::env::var_os("ZERUN_CANDIDATE_EXE").unwrap();
        std::fs::copy(standalone, &exe).unwrap();
        assert_version(&exe, "zerun", version);
        assert_eq!(
            windows::incomplete_install(&exe, version).as_deref(),
            Some(reference)
        );
        assert!(matches!(
            windows::complete_install(base, reference, version)
                .await
                .unwrap(),
            windows::Completion::Installed
        ));
    }
    assert!(windows::incomplete_install(&exe, version).is_none());
    assert_version(&exe, "zerun", version);
    let files = windows::version_files(reference, version);
    assert_version(
        &files.join("engine/zerun-engine.exe"),
        "zerun-engine",
        version,
    );
    assert!(files.join("ui/app.luau").is_file());
    assert!(files.join("plugins-builtin").is_dir());
}

fn assert_version(exe: &Path, binary: &str, version: &str) {
    let output = Command::new(exe).arg("--version").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("{binary} {version}")
    );
}
