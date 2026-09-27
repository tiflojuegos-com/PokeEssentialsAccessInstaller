use std::sync::OnceLock;

use serde::Deserialize;

use super::paths::tree_url;
use super::source::ContentEntry;

#[derive(Debug, Clone, Deserialize)]
struct Tree {
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    tree: Vec<TreeNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct TreeNode {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    sha: String,
}

pub fn client() -> Result<reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent("PokeEssentialsAccessLauncher")
                .timeout(std::time::Duration::from_secs(120))
                .pool_max_idle_per_host(16)
                .build()
                .map_err(|e| format!("cliente HTTP: {}", e))
        })
        .clone()
}

fn parse_tree(text: &str) -> Result<Tree, String> {
    serde_json::from_str(text).map_err(|e| format!("parseo arbol: {}", e))
}

fn blobs(tree: &Tree) -> Vec<ContentEntry> {
    tree.tree
        .iter()
        .filter(|n| n.kind == "blob")
        .map(|n| ContentEntry { path: n.path.clone(), sha: n.sha.clone() })
        .collect()
}

pub fn fetch_tree() -> Result<Vec<ContentEntry>, String> {
    let bytes = download_bytes(&tree_url())?;
    let tree = parse_tree(&String::from_utf8_lossy(&bytes))?;
    if tree.truncated {
        return Err("err_tree_truncated".to_string());
    }
    Ok(blobs(&tree))
}

pub fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = client()?
        .get(url)
        .send()
        .map_err(|e| crate::i18n::err_key("err_download", &e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 429 || (status.as_u16() == 403 && is_rate_limited(resp.headers())) {
        return Err(rate_limit_message(resp.headers()));
    }
    if !status.is_success() {
        return Err(crate::i18n::err_key("err_download_status", &status.to_string()));
    }
    resp.bytes()
        .map(|b| b.to_vec())
        .map_err(|e| crate::i18n::err_key("err_download", &e.to_string()))
}

fn is_rate_limited(headers: &reqwest::header::HeaderMap) -> bool {
    if headers.get("retry-after").is_some() {
        return true;
    }
    headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "0")
        .unwrap_or(false)
}

fn rate_limit_message(headers: &reqwest::header::HeaderMap) -> String {
    match retry_minutes(headers) {
        Some(m) => crate::i18n::err_key("err_rate_limited", &m.to_string()),
        None => "err_rate_limited_short".to_string(),
    }
}

fn retry_minutes(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    if let Some(secs) = header_u64(headers, "retry-after") {
        return Some(secs.div_ceil(60).max(1));
    }
    let reset = header_u64(headers, "x-ratelimit-reset")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let wait = reset.saturating_sub(now);
    Some(wait.div_ceil(60).max(1))
}

fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_minutes_from_retry_after() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "90".parse().unwrap());
        assert_eq!(retry_minutes(&h), Some(2));
    }

    #[test]
    fn retry_minutes_from_reset_in_future() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let reset = now + 130;
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-ratelimit-reset", reset.to_string().parse().unwrap());
        let m = retry_minutes(&h).unwrap();
        assert!(m >= 2 && m <= 3);
    }

    #[test]
    fn the_rate_limit_message_is_translated_with_its_minutes() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "90".parse().unwrap());
        let shown = crate::i18n::I18n::new("fr").t_err(&rate_limit_message(&h));
        assert!(shown.contains("2 minutes"), "{}", shown);
        assert!(!shown.contains("err_rate_limited"));
        let short = crate::i18n::I18n::new("en").t_err(&rate_limit_message(&reqwest::header::HeaderMap::new()));
        assert!(short.to_lowercase().contains("wait"), "{}", short);
    }

    #[test]
    fn retry_minutes_none_without_headers() {
        let h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_minutes(&h), None);
    }

    #[test]
    fn is_rate_limited_true_with_evidence() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "30".parse().unwrap());
        assert!(is_rate_limited(&h));

        let mut h2 = reqwest::header::HeaderMap::new();
        h2.insert("x-ratelimit-remaining", "0".parse().unwrap());
        assert!(is_rate_limited(&h2));
    }

    #[test]
    fn is_rate_limited_false_for_plain_403() {
        let h = reqwest::header::HeaderMap::new();
        assert!(!is_rate_limited(&h));

        let mut h2 = reqwest::header::HeaderMap::new();
        h2.insert("x-ratelimit-remaining", "42".parse().unwrap());
        assert!(!is_rate_limited(&h2));
    }

    #[test]
    fn the_tree_gives_each_file_with_its_git_sha_and_no_folder() {
        let json = r#"{"truncated":false,"tree":[
            {"path":"core","type":"tree","sha":"t0"},
            {"path":"core/nav","type":"tree","sha":"t1"},
            {"path":"core/nav/locator.rb","type":"blob","sha":"s1"},
            {"path":"games/pokemon_z/menu.rb","type":"blob","sha":"s2"},
            {"path":"vendor/sub","type":"commit","sha":"c1"}
        ]}"#;
        let entries = blobs(&parse_tree(json).unwrap());
        assert_eq!(
            entries,
            vec![
                ContentEntry { path: "core/nav/locator.rb".into(), sha: "s1".into() },
                ContentEntry { path: "games/pokemon_z/menu.rb".into(), sha: "s2".into() },
            ]
        );
    }

    #[test]
    #[ignore]
    fn net_probe_repo_reachable() {
        let vt = super::download_bytes(&super::super::paths::raw_url("version.json")).expect("version.json");
        let v = String::from_utf8_lossy(&vt);
        println!("PROBE version.json = {}", v.trim());
        assert!(v.contains("version"));
    }

    #[test]
    #[ignore]
    fn net_probe_tree_one_request() {
        let files = super::fetch_tree().expect("fetch_tree");
        println!("PROBE tree files = {}", files.len());
        let has_core = files.iter().any(|f| f.path.starts_with("core/"));
        let has_game = files.iter().any(|f| f.path.starts_with("games/pokemon_z/"));
        let all_have_sha = files.iter().all(|f| !f.sha.is_empty());
        for f in files.iter().take(5) {
            println!("  {} sha={}", f.path, &f.sha[..f.sha.len().min(8)]);
        }
        assert!(has_core, "no core files");
        assert!(has_game, "no pokemon_z game files");
        assert!(all_have_sha, "some file has empty sha");
    }

    #[test]
    #[ignore]
    fn net_probe_parallel_matches_sequential_and_is_faster() {
        use std::time::Instant;
        let files = super::fetch_tree().expect("fetch_tree");
        let sample: Vec<String> = files
            .iter()
            .filter(|f| f.path.starts_with("core/"))
            .take(24)
            .map(|f| super::super::paths::raw_url(&f.path))
            .collect();
        assert!(sample.len() >= 12, "not enough files to compare");

        let t0 = Instant::now();
        let seq: Vec<Vec<u8>> = sample.iter().map(|u| super::download_bytes(u).unwrap()).collect();
        let seq_ms = t0.elapsed().as_millis();

        let t1 = Instant::now();
        let mut par: Vec<Option<Vec<u8>>> = vec![None; sample.len()];
        for chunk in (0..sample.len()).collect::<Vec<_>>().chunks(8) {
            let handles: Vec<_> = chunk
                .iter()
                .map(|&i| {
                    let u = sample[i].clone();
                    std::thread::spawn(move || (i, super::download_bytes(&u).unwrap()))
                })
                .collect();
            for h in handles {
                let (i, data) = h.join().unwrap();
                par[i] = Some(data);
            }
        }
        let par_ms = t1.elapsed().as_millis();

        for (i, s) in seq.iter().enumerate() {
            assert_eq!(par[i].as_ref().unwrap(), s, "byte mismatch at {}", i);
        }
        println!("PROBE {} files: secuencial={}ms paralelo={}ms", sample.len(), seq_ms, par_ms);
        assert!(par_ms <= seq_ms, "paralelo no fue mas rapido");
    }
}
