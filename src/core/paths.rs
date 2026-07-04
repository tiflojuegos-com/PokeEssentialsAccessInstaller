use std::path::PathBuf;

pub const GITHUB_OWNER: &str = "tiflojuegos-com";
pub const GITHUB_REPO: &str = "PokeEssentialsAccess";
pub const GITHUB_BRANCH: &str = "main";

pub fn raw_url(path: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        GITHUB_OWNER, GITHUB_REPO, GITHUB_BRANCH, path
    )
}

pub fn tree_url() -> String {
    format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
        GITHUB_OWNER, GITHUB_REPO, GITHUB_BRANCH
    )
}

pub fn launcher_config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("PokeEssentialsAccessLauncher")
}

pub fn launcher_config_file() -> PathBuf {
    launcher_config_dir().join("config.json")
}

pub fn accessibility_dir(game_dir: &std::path::Path) -> PathBuf {
    game_dir.join("accessibility")
}

pub fn installed_file(game_dir: &std::path::Path) -> PathBuf {
    accessibility_dir(game_dir).join("data").join("installed.json")
}
