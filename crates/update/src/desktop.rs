//! The desktop app's own updates, for whichever interface it has (the GPUI
//! app's `crates/ui/src/app_update.rs`, the zgx interface's `src/about.rs`):
//! what both do the same, apart from how they draw it.
//!
//! The lifecycle around these (Sparkle's, VS Code's): a found release
//! downloads in the background ([`stage_offer`]), "Restart to update" installs
//! it and relaunches ([`InstallKind::apply_desktop`] with `relaunch`), and one
//! nobody restarted for installs when the app quits (without).

use std::path::{Path, PathBuf};

use crate::{Channel, InstallKind, Offer, app_version, bundle_engine_version, current_version, channel, fetch_latest, offer};

/// The packaged engine's version: Beta packages carry the matching build on
/// every platform. A macOS bundle also records it for older app packages.
pub fn engine_version(install: &InstallKind) -> Option<String> {
    match install {
        InstallKind::MacApp { bundle } => bundle_engine_version(bundle),
        _ => Some(current_version().to_owned()),
    }
}

/// What the selected channel offers this app now, with the release itself:
/// the feed asked again, so a long-lived status still lands on the newest
/// release (and that release's checksums).
pub async fn current_offer(edge_url: &str, install: &InstallKind) -> anyhow::Result<(Offer, crate::Manifest)> {
    let manifest = fetch_latest(edge_url).await?;
    let running_engine = engine_version(install);
    let offer = offer(
        &manifest,
        app_version(),
        manifest.channel,
        running_engine.as_deref(),
        channel::macos_version(),
    );
    Ok((offer, manifest))
}

/// Download, verify and stage what the selected channel offers this app (a
/// newer release, or the other channel's after a switch): its version and
/// the staged bundle. An offer that's gone or held is an error, in words for
/// the person.
pub async fn stage_offer(
    edge_url: &str,
    data_dir: &Path,
    install: &InstallKind,
) -> anyhow::Result<(String, PathBuf)> {
    stage_offer_with_progress(edge_url, data_dir, install, None).await
}

/// Stage the current offer while reporting verified-file download progress.
pub async fn stage_offer_with_progress(
    edge_url: &str,
    data_dir: &Path,
    install: &InstallKind,
    progress: Option<&tokio::sync::watch::Sender<crate::DownloadProgress>>,
) -> anyhow::Result<(String, PathBuf)> {
    let (offer, manifest) = current_offer(edge_url, install).await?;
    match offer {
        Offer::Newer | Offer::Switch => {}
        Offer::Held(why) => anyhow::bail!(why),
        Offer::Current => anyhow::bail!("the release feed no longer offers a newer version"),
    }
    let staged = install.stage_desktop_with_progress(edge_url, &manifest, data_dir, progress).await?;
    Ok((manifest.version, staged))
}

/// Choose the installed app's channel. macOS also enforces its Beta system
/// requirement. Leaving Beta is always allowed.
pub fn choose_channel(data_dir: &Path, beta: bool) -> Result<Channel, String> {
    if beta && let Some(why) = channel::beta_unavailable() {
        return Err(why);
    }
    let chosen = if beta { Channel::Beta } else { Channel::Stable };
    channel::select(data_dir, chosen).map_err(|err| format!("Couldn't save the choice: {err}"))?;
    tracing::info!(channel = chosen.as_str(), "update channel chosen");
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_mac_beta_updates_also_wait_for_the_stable_engine_to_catch_up() {
        let running = engine_version(&InstallKind::Managed { app_root: "/example/app".into() }).unwrap();
        let stable = crate::Manifest { version: "0.1.0".into(), ..Default::default() };
        assert!(matches!(offer(&stable, "99.0.0-beta.1", Channel::Stable, Some(&running), None), Offer::Held(_)));
        let caught_up = crate::Manifest { version: "99.0.0".into(), ..Default::default() };
        assert_eq!(offer(&caught_up, "99.0.0-beta.1", Channel::Stable, Some(&running), None), Offer::Newer);
    }

    #[test]
    fn leaving_beta_is_always_allowed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(choose_channel(dir.path(), false), Ok(Channel::Stable));
        assert_eq!(channel::read(dir.path()).channel, Channel::Stable);
    }

    /// A test binary isn't an installed desktop app: beta is refused, in words.
    #[test]
    fn beta_needs_an_app_install() {
        let dir = tempfile::tempdir().unwrap();
        let refused = choose_channel(dir.path(), true).unwrap_err();
        assert!(refused.contains("installed Zerun desktop app"), "{refused}");
        assert_eq!(channel::read(dir.path()).channel, Channel::Stable);
    }
}
