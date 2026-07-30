//! PiDeck installation update boundary.
//!
//! Velopack performs blocking network, package, and process work. Call these
//! functions only from a background executor.

use velopack::{Error, UpdateCheck, UpdateInfo, UpdateManager, sources::HttpSource};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_ID: &str = "PiDeck";
const UPDATE_BASE_URL: &str = "https://github.com/dev4st3d/pideck/releases/latest/download";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Current,
    UpdateAvailable { version: String },
    NotInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    AlreadyCurrent,
    NotInstalled,
    RestartScheduled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFailure {
    message: &'static str,
}

impl UpdateFailure {
    pub fn message(&self) -> &'static str {
        self.message
    }

    fn new(message: &'static str) -> Self {
        Self { message }
    }
}

#[derive(Debug, Clone, Copy)]
enum UpdateStage {
    Check,
    Download,
    Restart,
}

pub fn check_for_update() -> Result<CheckOutcome, UpdateFailure> {
    let Some(manager) = update_manager(UpdateStage::Check)? else {
        return Ok(CheckOutcome::NotInstalled);
    };

    let check = manager
        .check_for_updates()
        .map_err(|error| map_error(UpdateStage::Check, error))?;
    interpret_check(check)
}

/// Downloads and verifies the latest package, then starts the external updater.
/// The caller must close PiDeck after `RestartScheduled` so the updater can swap
/// the installed files and relaunch the application.
pub fn download_and_schedule_update() -> Result<InstallOutcome, UpdateFailure> {
    let Some(manager) = update_manager(UpdateStage::Check)? else {
        return Ok(InstallOutcome::NotInstalled);
    };

    let check = manager
        .check_for_updates()
        .map_err(|error| map_error(UpdateStage::Check, error))?;
    let UpdateCheck::UpdateAvailable(update) = check else {
        return Ok(InstallOutcome::AlreadyCurrent);
    };
    validate_update(&update)?;

    manager
        .download_updates(&update, None)
        .map_err(|error| map_error(UpdateStage::Download, error))?;
    manager
        .wait_exit_then_apply_updates(&update, false, true, Vec::<String>::new())
        .map_err(|error| map_error(UpdateStage::Restart, error))?;

    Ok(InstallOutcome::RestartScheduled)
}

fn update_manager(stage: UpdateStage) -> Result<Option<UpdateManager>, UpdateFailure> {
    let source = HttpSource::new(UPDATE_BASE_URL);
    match UpdateManager::new(source, None, None) {
        Ok(manager) => Ok(Some(manager)),
        Err(Error::NotInstalled(_)) => Ok(None),
        Err(error) => Err(map_error(stage, error)),
    }
}

fn interpret_check(check: UpdateCheck) -> Result<CheckOutcome, UpdateFailure> {
    match check {
        UpdateCheck::UpdateAvailable(update) => {
            validate_update(&update)?;
            Ok(CheckOutcome::UpdateAvailable {
                version: update.TargetFullRelease.Version,
            })
        }
        UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => Ok(CheckOutcome::Current),
    }
}

fn validate_update(update: &UpdateInfo) -> Result<(), UpdateFailure> {
    if update
        .TargetFullRelease
        .PackageId
        .eq_ignore_ascii_case(APP_ID)
    {
        Ok(())
    } else {
        Err(UpdateFailure::new(
            "GitHub returned a package that does not belong to PiDeck. Nothing was installed.",
        ))
    }
}

fn map_error(stage: UpdateStage, error: Error) -> UpdateFailure {
    eprintln!("PiDeck update {stage:?} failed: {error}");
    let message = match (stage, error) {
        (UpdateStage::Check, Error::Network(_)) => {
            "PiDeck couldn't reach GitHub. Check your connection and try again."
        }
        (UpdateStage::Check, _) => "PiDeck couldn't check for updates. Try again in a moment.",
        (UpdateStage::Download, Error::Network(_)) => {
            "The update couldn't be downloaded. Check your connection and try again."
        }
        (UpdateStage::Download, _) => {
            "The update couldn't be verified or prepared. Nothing was installed."
        }
        (UpdateStage::Restart, _) => {
            "The update was downloaded, but PiDeck couldn't start the updater. Restart PiDeck and try again."
        }
    };
    UpdateFailure::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use velopack::VelopackAsset;

    fn update(package_id: &str, version: &str) -> UpdateInfo {
        UpdateInfo {
            TargetFullRelease: VelopackAsset {
                PackageId: package_id.to_owned(),
                Version: version.to_owned(),
                ..VelopackAsset::default()
            },
            ..UpdateInfo::default()
        }
    }

    #[test]
    fn check_accepts_only_pideck_packages() {
        assert_eq!(
            interpret_check(UpdateCheck::UpdateAvailable(update("PiDeck", "1.2.3"))).unwrap(),
            CheckOutcome::UpdateAvailable {
                version: "1.2.3".to_owned()
            }
        );
        assert!(
            interpret_check(UpdateCheck::UpdateAvailable(update("OtherApp", "9.0.0"))).is_err()
        );
    }

    #[test]
    fn empty_and_current_feeds_are_current() {
        assert_eq!(
            interpret_check(UpdateCheck::RemoteIsEmpty).unwrap(),
            CheckOutcome::Current
        );
        assert_eq!(
            interpret_check(UpdateCheck::NoUpdateAvailable).unwrap(),
            CheckOutcome::Current
        );
    }
}
