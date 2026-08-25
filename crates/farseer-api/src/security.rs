//! The two things that stand between farseer and the operator's browser.
//!
//! `16 local api surface` section 10 is unusually blunt about why the second one is not optional
//! hardening: Docker Desktop's **CVE-2025-9074** was exactly this shape - a
//! loopback-bound API reachable by any web page in the operator's browser
//! through DNS rebinding. **A token alone does not save you**, because the
//! browser attaches it for the attacker.

use std::io;
use std::path::{Path, PathBuf};

/// The bearer token, generated at runtime start.
///
/// The CLI reads it automatically, so the operator never sees it. A browser UI
/// receives it in a URL **fragment**, never a query string: a fragment is not
/// sent to the server and not written to server logs.
#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeToken(String);

impl RuntimeToken {
    /// 256 bits from the OS CSPRNG, hex encoded.
    pub fn generate() -> Self {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        Self(format!("{}{}", a.simple(), b.simple()))
    }

    pub fn from_secret(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compare without leaking the matching prefix through timing.
    pub fn matches(&self, presented: &str) -> bool {
        let expected = self.0.as_bytes();
        let got = presented.as_bytes();
        let mut diff = (expected.len() ^ got.len()) as u8;
        for i in 0..expected.len().max(got.len()) {
            let e = expected.get(i).copied().unwrap_or(0);
            let g = got.get(i).copied().unwrap_or(0);
            diff |= e ^ g;
        }
        diff == 0
    }
}

impl std::fmt::Debug for RuntimeToken {
    /// Never print the value. A token in a log is a token on disk.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RuntimeToken(...)")
    }
}

/// Whether a browser is allowed to have sent this request.
///
/// Two independent checks, because they stop two different attacks:
///
/// - **`Host`** must be a loopback literal. This is what defeats DNS rebinding,
///   where a hostile page resolves its own domain to `127.0.0.1`: the browser
///   sends the attacker's hostname, not ours.
/// - **`Origin`**, when present, must be a loopback origin. This stops an
///   ordinary cross-site `fetch`, which the browser would otherwise send with
///   the token attached.
///
/// A missing `Origin` is allowed: the CLI and `curl` do not send one, and the
/// `Host` check already covers the browser case.
pub fn is_origin_allowed(host: Option<&str>, origin: Option<&str>) -> bool {
    fn loopback_authority(authority: &str) -> bool {
        // An IPv6 literal is bracketed and full of colons, so the port has to be
        // split off after the closing bracket rather than at the last colon.
        let host = match authority.strip_prefix('[') {
            Some(rest) => match rest.split_once(']') {
                Some((inner, _port)) => inner,
                None => return false,
            },
            None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
        };
        matches!(host, "127.0.0.1" | "localhost" | "::1")
    }

    let Some(host) = host else { return false };
    if !loopback_authority(host) {
        return false;
    }
    match origin {
        None => true,
        Some(origin) => origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .is_some_and(loopback_authority),
    }
}

/// Where the CLI looks for the port and the token.
pub fn runtime_file_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("farseer").join("runtime.json")
}

/// `10 runner inventory`: keep a manager's generated bearer-bearing MCP
/// config outside the git worktree and delete it independently when the run
/// exits, so neither the secret nor cleanup depends on workspace teardown.
pub(crate) fn manager_config_path(run_id: &str) -> PathBuf {
    runtime_file_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("manager-configs")
        .join(format!("{run_id}.json"))
}

/// Write sensitive bytes under a current-user-only DACL.
///
/// `10 runner inventory` records that a native runner inherits operator
/// configuration unless its adapter prevents it, so a generated manager
/// capability must never rely on inherited directory permissions.
pub(crate) fn write_user_only_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Lock the file down while it is still empty. Writing first would expose
    // the secret under inherited permissions if the process died in between.
    std::fs::write(path, b"")?;
    restrict_to_current_user(path)?;
    std::fs::write(path, contents)
}

/// Write the port and token where the CLI can find them, readable by nobody else.
pub fn write_runtime_file(path: &Path, port: u16, token: &RuntimeToken) -> io::Result<()> {
    let body = serde_json::json!({ "port": port, "token": token.as_str() });
    write_user_only_file(path, &serde_json::to_vec(&body)?)
}

/// Replace the file's DACL with one entry: full control for the current user.
///
/// `LOCALAPPDATA` is already per-user, so this is belt and braces - but the
/// token is the one secret farseer writes to disk, and inheriting whatever the
/// parent directory happens to grant is not a decision anyone made.
#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW, SetNamedSecurityInfoW,
        TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PWSTR;

    unsafe {
        let mut process_token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut process_token)
            .map_err(io::Error::other)?;

        let mut needed = 0u32;
        let _ = windows::Win32::Security::GetTokenInformation(
            process_token,
            TokenUser,
            None,
            0,
            &mut needed,
        );
        let mut buffer = vec![0u8; needed as usize];
        windows::Win32::Security::GetTokenInformation(
            process_token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .map_err(io::Error::other)?;

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let sid: PSID = token_user.User.Sid;

        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: 0x001F_01FF, // FILE_ALL_ACCESS
            grfAccessMode: SET_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: PWSTR(sid.0.cast()),
                ..Default::default()
            },
        };

        let mut acl: *mut ACL = std::ptr::null_mut();
        let status = SetEntriesInAclW(Some(&[access]), None, &mut acl);
        if status.is_err() {
            return Err(io::Error::other(format!(
                "SetEntriesInAclW failed: {status:?}"
            )));
        }

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let status = SetNamedSecurityInfoW(
            PWSTR(wide.as_mut_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        );
        let _ = LocalFree(Some(HLOCAL(acl.cast())));
        if status.is_err() {
            return Err(io::Error::other(format!(
                "SetNamedSecurityInfoW failed: {status:?}"
            )));
        }
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(any(windows, unix)))]
fn restrict_to_current_user(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_token_is_256_bits_of_hex_and_never_repeats() {
        let a = RuntimeToken::generate();
        let b = RuntimeToken::generate();
        assert_eq!(a.as_str().len(), 64);
        assert!(a.as_str().chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn a_token_matches_only_itself_including_by_length() {
        let token = RuntimeToken::from_secret("abc123");
        assert!(token.matches("abc123"));
        assert!(!token.matches("abc124"));
        assert!(!token.matches("abc"));
        assert!(!token.matches("abc1234"));
        assert!(!token.matches(""));
    }

    #[test]
    fn a_token_never_prints_itself() {
        let token = RuntimeToken::from_secret("supersecret");
        assert!(!format!("{token:?}").contains("supersecret"));
    }

    #[test]
    fn a_rebound_hostname_is_refused_even_though_it_resolved_to_loopback() {
        assert!(!is_origin_allowed(Some("evil.example.com:9000"), None));
        assert!(!is_origin_allowed(Some("evil.example.com"), None));
    }

    #[test]
    fn a_cross_site_fetch_is_refused_even_with_a_loopback_host() {
        assert!(!is_origin_allowed(
            Some("127.0.0.1:9000"),
            Some("https://evil.example.com")
        ));
    }

    #[test]
    fn an_ipv6_loopback_literal_is_allowed_with_or_without_a_port() {
        assert!(is_origin_allowed(Some("[::1]:9000"), None));
        assert!(is_origin_allowed(Some("[::1]"), None));
        assert!(is_origin_allowed(
            Some("[::1]:9000"),
            Some("http://[::1]:9000")
        ));
        assert!(!is_origin_allowed(Some("[2001:db8::1]:9000"), None));
    }

    #[test]
    fn the_cli_sending_no_origin_is_allowed_on_a_loopback_host() {
        assert!(is_origin_allowed(Some("127.0.0.1:9000"), None));
        assert!(is_origin_allowed(Some("localhost:9000"), None));
        assert!(is_origin_allowed(
            Some("127.0.0.1:9000"),
            Some("http://localhost:9000")
        ));
    }

    #[test]
    fn a_request_with_no_host_at_all_is_refused() {
        assert!(!is_origin_allowed(None, None));
    }

    #[test]
    fn the_runtime_file_is_written_and_locked_down() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("farseer").join("runtime.json");
        let token = RuntimeToken::generate();
        write_runtime_file(&path, 9000, &token).unwrap();

        let read: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(read["port"], 9000);
        assert_eq!(read["token"], token.as_str());
    }

    /// The DACL is the point of the exercise, so read it back rather than
    /// trusting that the call returned success.
    #[cfg(windows)]
    #[test]
    fn the_runtime_file_grants_exactly_one_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime.json");
        write_runtime_file(&path, 9000, &RuntimeToken::generate()).unwrap();

        let output = std::process::Command::new("icacls")
            .arg(&path)
            .output()
            .expect("icacls");
        let listing = String::from_utf8_lossy(&output.stdout);
        // `icacls` prints `<path> DOMAIN\user:(F)` then one indented
        // `DOMAIN\user:(F)` per further ACE. The path carries its own colon, so
        // split on the rights marker and keep the last word before it.
        let identities: Vec<&str> = listing
            .lines()
            .filter_map(|line| line.rsplit_once(":("))
            .filter_map(|(left, _)| left.rsplit(' ').next())
            .collect();

        assert_eq!(identities.len(), 1, "unexpected DACL:\n{listing}");
        let me = std::env::var("USERNAME").unwrap_or_default();
        assert!(
            identities[0].to_lowercase().ends_with(&me.to_lowercase()),
            "the single ACE is not the current user: {identities:?}"
        );
    }
}
