use std::sync::OnceLock;

use serde::Deserialize;

use super::paths::{raw_url, tree_url};

#[derive(Debug, Clone, Deserialize)]
pub struct ContentEntry {
    #[allow(dead_code)]
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub download_url: Option<String>,
}

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

fn under_prefix(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| path == p || path.starts_with(&format!("{}/", p)))
}

fn tree_to_entries(tree: &Tree, prefixes: &[String]) -> Vec<ContentEntry> {
    tree.tree
        .iter()
        .filter(|n| n.kind == "blob" && under_prefix(&n.path, prefixes))
        .map(|n| ContentEntry {
            name: n.path.rsplit('/').next().unwrap_or(&n.path).to_string(),
            path: n.path.clone(),
            kind: "file".to_string(),
            sha: n.sha.clone(),
            download_url: Some(raw_url(&n.path)),
        })
        .collect()
}

pub fn walk_tree(prefixes: &[String]) -> Result<Vec<ContentEntry>, String> {
    let bytes = download_bytes(&tree_url())?;
    let tree = parse_tree(&String::from_utf8_lossy(&bytes))?;
    if tree.truncated {
        return Err("el arbol del repositorio es demasiado grande (respuesta truncada)".to_string());
    }
    Ok(tree_to_entries(&tree, prefixes))
}

pub fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = client()?
        .get(url)
        .send()
        .map_err(|e| format!("descarga: {}", e))?;
    let status = resp.status();
    if status.as_u16() == 429 || (status.as_u16() == 403 && is_rate_limited(resp.headers())) {
        return Err(rate_limit_message(resp.headers()));
    }
    if !status.is_success() {
        return Err(format!("descarga estado {}", status));
    }
    resp.bytes()
        .map(|b| b.to_vec())
        .map_err(|e| format!("lectura descarga: {}", e))
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
        Some(m) => format!(
            "GitHub ha limitado las peticiones. Espera unos {} minutos y vuelve a intentarlo.",
            m
        ),
        None => "GitHub ha limitado las peticiones. Espera un rato y vuelve a intentarlo.".to_string(),
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
    fn parse_content_entry() {
        let e: ContentEntry = serde_json::from_str(r#"{"name":"locator.rb","path":"core/nav/locator.rb","type":"file","sha":"abc","download_url":"https://x/locator.rb"}"#).unwrap();
        assert_eq!(e.kind, "file");
        assert_eq!(e.path, "core/nav/locator.rb");
        assert_eq!(e.download_url.unwrap(), "https://x/locator.rb");
    }

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
    fn tree_filters_by_prefix_and_keeps_blobs() {
        let json = r#"{"truncated":false,"tree":[
            {"path":"core","type":"tree","sha":"t0"},
            {"path":"core/nav","type":"tree","sha":"t1"},
            {"path":"core/nav/locator.rb","type":"blob","sha":"s1"},
            {"path":"games/pokemon_z/menu.rb","type":"blob","sha":"s2"},
            {"path":"games/reminiscencia/menu.rb","type":"blob","sha":"s3"},
            {"path":"README.md","type":"blob","sha":"s4"}
        ]}"#;
        let tree = parse_tree(json).unwrap();
        let prefixes = vec!["core".to_string(), "games/pokemon_z".to_string()];
        let entries = tree_to_entries(&tree, &prefixes);
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"core/nav/locator.rb"));
        assert!(paths.contains(&"games/pokemon_z/menu.rb"));
        assert!(!paths.contains(&"games/reminiscencia/menu.rb"));
        assert!(!paths.contains(&"README.md"));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn tree_entry_has_raw_download_and_git_sha() {
        let json = r#"{"truncated":false,"tree":[{"path":"core/nav/locator.rb","type":"blob","sha":"deadbeef"}]}"#;
        let tree = parse_tree(json).unwrap();
        let e = &tree_to_entries(&tree, &["core".to_string()])[0];
        assert_eq!(e.sha, "deadbeef");
        assert_eq!(e.name, "locator.rb");
        assert_eq!(e.kind, "file");
        assert!(e.download_url.as_ref().unwrap().starts_with("https://raw.githubusercontent.com/"));
    }

    #[test]
    fn under_prefix_does_not_match_partial_dir_name() {
        let prefixes = vec!["core".to_string()];
        assert!(under_prefix("core/x.rb", &prefixes));
        assert!(under_prefix("core", &prefixes));
        assert!(!under_prefix("corefoo/x.rb", &prefixes));
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
        let prefixes = vec!["core".to_string(), "games/pokemon_z".to_string(), "loader".to_string()];
        let files = super::walk_tree(&prefixes).expect("walk_tree");
        println!("PROBE tree files = {}", files.len());
        let has_core = files.iter().any(|f| f.path.starts_with("core/"));
        let has_game = files.iter().any(|f| f.path.starts_with("games/pokemon_z/"));
        let all_have_sha = files.iter().all(|f| !f.sha.is_empty());
        let all_raw = files.iter().all(|f| f.download_url.as_deref().unwrap_or("").starts_with("https://raw."));
        for f in files.iter().take(5) {
            println!("  {} sha={}", f.path, &f.sha[..f.sha.len().min(8)]);
        }
        assert!(has_core, "no core files");
        assert!(has_game, "no pokemon_z game files");
        assert!(all_have_sha, "some file has empty sha");
        assert!(all_raw, "some download_url is not raw");
    }

    #[test]
    #[ignore]
    fn net_probe_parallel_matches_sequential_and_is_faster() {
        use std::time::Instant;
        let files = super::walk_tree(&["core".to_string()]).expect("walk_tree");
        let sample: Vec<String> = files.iter().take(24).map(|f| f.download_url.clone().unwrap()).collect();
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
