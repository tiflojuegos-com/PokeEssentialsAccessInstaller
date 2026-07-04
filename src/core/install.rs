use sha1::{Digest, Sha1};

pub fn git_blob_sha1(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", data.len()).as_bytes());
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn needs_update(local_sha: Option<&String>, remote_sha: &str) -> bool {
    match local_sha {
        Some(s) => !s.eq_ignore_ascii_case(remote_sha),
        None => true,
    }
}

pub fn is_user_data(rel_path: &str) -> bool {
    let p = rel_path.replace('\\', "/").to_lowercase();
    p.starts_with("data/")
        || p == "installed.json"
        || p.ends_with("/settings.ini")
        || p.ends_with("/tags.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_blob_sha_matches_git() {
        assert_eq!(git_blob_sha1(b"hello\n"), "ce013625030ba8dba906f756967f9e9ca394464a");
        assert_eq!(git_blob_sha1(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn needs_update_logic() {
        assert!(needs_update(None, "abc"));
        assert!(needs_update(Some(&"aaa".to_string()), "bbb"));
        assert!(!needs_update(Some(&"ABC".to_string()), "abc"));
    }

    #[test]
    fn user_data_preserved() {
        assert!(is_user_data("data/settings.ini"));
        assert!(is_user_data("data/installed.json"));
        assert!(!is_user_data("core/nav/locator.rb"));
        assert!(!is_user_data("lang/es.txt"));
    }
}
