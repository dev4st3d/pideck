//! Installed Windows update boundary.
//!
//! These functions are blocking and must only be called from a background executor.

#[cfg(windows)]
use velopack::{Error, UpdateCheck, UpdateManager, VelopackAsset, sources::HttpSource};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(windows)]
const UPDATE_BASE_URL: &str = "https://github.com/dev4st3d/pideck/releases/latest/download";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Current,
    Available { version: String },
    Prepared { version: String },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareOutcome {
    Current,
    Prepared { version: String },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleOutcome {
    Scheduled,
    Unavailable,
    NotPrepared,
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

/// Checks the installed package's feed. A verified package waiting for restart wins so a
/// previously cancelled restart remains retryable without another download.
pub fn check_for_update() -> Result<CheckOutcome, UpdateFailure> {
    #[cfg(windows)]
    {
        let Some(manager) = update_manager(Stage::Check)? else {
            return Ok(CheckOutcome::Unavailable);
        };
        if let Some(asset) = pending_update(&manager)? {
            return Ok(CheckOutcome::Prepared {
                version: asset.Version,
            });
        }
        match manager
            .check_for_updates()
            .map_err(|error| map_error(Stage::Check, error))?
        {
            UpdateCheck::UpdateAvailable(update) => {
                validate_asset(&update.TargetFullRelease)?;
                Ok(CheckOutcome::Available {
                    version: update.TargetFullRelease.Version,
                })
            }
            UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => {
                Ok(CheckOutcome::Current)
            }
        }
    }
    #[cfg(not(windows))]
    {
        Ok(CheckOutcome::Unavailable)
    }
}

/// Re-checks, downloads, and lets Velopack verify the package. It deliberately does not
/// launch Update.exe; scheduling is a separate final lifecycle action.
pub fn prepare_update() -> Result<PrepareOutcome, UpdateFailure> {
    #[cfg(windows)]
    {
        let Some(manager) = update_manager(Stage::Check)? else {
            return Ok(PrepareOutcome::Unavailable);
        };
        if let Some(asset) = pending_update(&manager)? {
            return Ok(PrepareOutcome::Prepared {
                version: asset.Version,
            });
        }
        let check = manager
            .check_for_updates()
            .map_err(|error| map_error(Stage::Check, error))?;
        let UpdateCheck::UpdateAvailable(update) = check else {
            return Ok(PrepareOutcome::Current);
        };
        validate_asset(&update.TargetFullRelease)?;
        manager
            .download_updates(&update, None)
            .map_err(|error| map_error(Stage::Prepare, error))?;
        let Some(asset) = pending_update(&manager)? else {
            return Err(UpdateFailure::new(
                "The update could not be verified or prepared. Nothing was installed.",
            ));
        };
        if asset.Version != update.TargetFullRelease.Version {
            return Err(UpdateFailure::new(
                "The prepared update did not match the release that was checked. Try again.",
            ));
        }
        Ok(PrepareOutcome::Prepared {
            version: asset.Version,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(PrepareOutcome::Unavailable)
    }
}

/// Starts only Velopack's external wait-and-apply process for the verified cached package.
pub fn schedule_prepared_update(version: &str) -> Result<ScheduleOutcome, UpdateFailure> {
    #[cfg(windows)]
    {
        let Some(manager) = update_manager(Stage::Schedule)? else {
            return Ok(ScheduleOutcome::Unavailable);
        };
        let Some(asset) = pending_update(&manager)? else {
            return Ok(ScheduleOutcome::NotPrepared);
        };
        if asset.Version != version {
            return Ok(ScheduleOutcome::NotPrepared);
        }
        manager
            .wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new())
            .map_err(|error| map_error(Stage::Schedule, error))?;
        Ok(ScheduleOutcome::Scheduled)
    }
    #[cfg(not(windows))]
    {
        let _ = version;
        Ok(ScheduleOutcome::Unavailable)
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
enum Stage {
    Check,
    Prepare,
    Schedule,
}

#[cfg(windows)]
fn update_manager(stage: Stage) -> Result<Option<UpdateManager>, UpdateFailure> {
    match UpdateManager::new(HttpSource::new(UPDATE_BASE_URL), None, None) {
        Ok(manager) => Ok(Some(manager)),
        Err(Error::NotInstalled(_)) => Ok(None),
        Err(error) => Err(map_error(stage, error)),
    }
}

#[cfg(windows)]
fn pending_update(manager: &UpdateManager) -> Result<Option<VelopackAsset>, UpdateFailure> {
    let Some(asset) = manager.get_update_pending_restart() else {
        return Ok(None);
    };
    validate_asset(&asset)?;
    Ok(Some(asset))
}

fn package_id_is_valid(package_id: &str) -> bool {
    package_id.eq_ignore_ascii_case("PiDeck")
}

#[cfg(windows)]
fn validate_asset(asset: &VelopackAsset) -> Result<(), UpdateFailure> {
    if !package_id_is_valid(&asset.PackageId) {
        return Err(UpdateFailure::new(
            "The update package does not belong to Pideck. Nothing was installed.",
        ));
    }
    if asset.Version.trim().is_empty() || asset.FileName.trim().is_empty() {
        return Err(UpdateFailure::new(
            "The update package was incomplete. Nothing was installed.",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn map_error(stage: Stage, error: Error) -> UpdateFailure {
    eprintln!("Pideck update {stage:?} failed: {error}");
    let message = match (stage, error) {
        (Stage::Check, Error::Network(_)) => {
            "Pideck couldn't reach GitHub. Check your connection and try again."
        }
        (Stage::Check, _) => "Pideck couldn't check for updates. Try again in a moment.",
        (Stage::Prepare, Error::Network(_)) => {
            "The update couldn't be downloaded. Check your connection and try again."
        }
        (Stage::Prepare, _) => {
            "The update couldn't be verified or prepared. Nothing was installed."
        }
        (Stage::Schedule, _) => {
            "The update is ready, but Pideck couldn't start it. Try restarting to update again."
        }
    };
    UpdateFailure::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_identity_is_case_insensitive_and_rejects_other_products() {
        assert!(package_id_is_valid("PiDeck"));
        assert!(package_id_is_valid("pideck"));
        assert!(!package_id_is_valid("PiDeck Preview"));
        assert!(!package_id_is_valid("OtherApp"));
    }

    #[test]
    fn unpackaged_platforms_are_quietly_unavailable() {
        #[cfg(not(windows))]
        {
            assert_eq!(check_for_update().unwrap(), CheckOutcome::Unavailable);
            assert_eq!(prepare_update().unwrap(), PrepareOutcome::Unavailable);
            assert_eq!(
                schedule_prepared_update("9.9.9").unwrap(),
                ScheduleOutcome::Unavailable
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn validates_package_identity_and_required_asset_fields() {
        let valid = VelopackAsset {
            PackageId: "pideck".into(),
            Version: "1.2.3".into(),
            FileName: "PiDeck-1.2.3-full.nupkg".into(),
            ..VelopackAsset::default()
        };
        assert!(validate_asset(&valid).is_ok());
        assert!(
            validate_asset(&VelopackAsset {
                PackageId: "other".into(),
                ..valid.clone()
            })
            .is_err()
        );
        assert!(
            validate_asset(&VelopackAsset {
                Version: String::new(),
                ..valid
            })
            .is_err()
        );
    }
}
