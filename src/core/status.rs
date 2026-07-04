#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    Outdated,
}

pub fn compute(installed_version: Option<&str>, available_version: &str) -> GameStatus {
    match installed_version {
        None => GameStatus::NotInstalled,
        Some(v) => {
            if semver_parse(v) == semver_parse(available_version) {
                GameStatus::UpToDate
            } else if newer(available_version, v) {
                GameStatus::UpdateAvailable
            } else {
                GameStatus::Outdated
            }
        }
    }
}

fn newer(a: &str, b: &str) -> bool {
    match (semver_parse(a), semver_parse(b)) {
        (Some(va), Some(vb)) => va > vb,
        _ => a != b,
    }
}

fn semver_parse(s: &str) -> Option<Vec<u64>> {
    let s = s.trim_start_matches('v');
    let parts: Vec<u64> = s.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

pub fn status_key(s: GameStatus) -> &'static str {
    match s {
        GameStatus::NotInstalled => "status_notinstalled",
        GameStatus::UpToDate => "status_uptodate",
        GameStatus::UpdateAvailable => "status_update",
        GameStatus::Outdated => "status_outdated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_installed() {
        assert_eq!(compute(None, "0.1.0"), GameStatus::NotInstalled);
    }

    #[test]
    fn up_to_date() {
        assert_eq!(compute(Some("0.1.0"), "0.1.0"), GameStatus::UpToDate);
    }

    #[test]
    fn update_available() {
        assert_eq!(compute(Some("0.1.0"), "0.2.0"), GameStatus::UpdateAvailable);
        assert_eq!(compute(Some("0.1.0"), "0.1.1"), GameStatus::UpdateAvailable);
        assert_eq!(compute(Some("0.9.0"), "0.10.0"), GameStatus::UpdateAvailable);
    }

    #[test]
    fn outdated_when_installed_is_newer() {
        assert_eq!(compute(Some("0.3.0"), "0.2.0"), GameStatus::Outdated);
    }

    #[test]
    fn tolerates_v_prefix() {
        assert_eq!(compute(Some("v0.1.0"), "0.1.0"), GameStatus::UpToDate);
    }
}
