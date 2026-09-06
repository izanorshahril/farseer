//! Discover and authenticate the local farseer runtime before the UI connects.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const OWNER_CONVERGENCE_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_secs(1);
const REQUIRED_FEATURES: &[&str] = &["health", "sse", "commands"];

#[derive(Debug, Clone, Deserialize)]
pub struct Runtime {
    pub port: u16,
    pub token: String,
    pub runtime_id: String,
    pub data_dir_fingerprint: String,
    pub api_version: String,
    pub build_provenance: String,
    pub features: Vec<String>,
}

/// The daemon this shell is talking to.
pub struct Attached {
    pub runtime: Runtime,
    /// Keep the child handle alive, but do not tie daemon lifetime to the UI.
    _child: Option<Child>,
}

/// Read and authenticate the runtime named by the discovery file.
pub fn attach_existing(expected_data_dir: &str) -> Result<Option<Runtime>> {
    let path = farseer_api::security::runtime_file_path();
    let Some(runtime) = read_runtime_file(&path)? else {
        return Ok(None);
    };
    match verify_classified(&runtime, expected_data_dir) {
        Ok(runtime) => Ok(Some(runtime)),
        Err(VerifyError::Unauthenticated(error)) => {
            let _ = error;
            Ok(None)
        }
        Err(VerifyError::Incompatible(error)) => Err(error),
    }
}

/// Start a daemon and wait for its authenticated health response.
pub fn spawn(binary: &Path, cells: &Path, repo: &Path, record: &Path) -> Result<Attached> {
    let path = farseer_api::security::runtime_file_path();
    let previous = read_runtime_file(&path)?;
    let expected_data_dir =
        farseer_api::security::data_dir_fingerprint(record.parent().unwrap_or(record));
    let mut child = Command::new(binary)
        .arg("serve")
        .arg("--port")
        .arg("0")
        .arg("--cells")
        .arg(cells)
        .arg("--repo")
        .arg(repo)
        .arg("--record")
        .arg(record)
        .spawn()
        .with_context(|| format!("starting {}", binary.display()))?;

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut last_observation = "runtime file not published".to_owned();
    while Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .context("checking the farseer child during startup")?
        {
            // Two shells can observe the same missing or stale discovery file
            // and race to start a daemon.  The data-directory lease lets one
            // child win; give that owner a short publication window before
            // reporting the losing child exit, then attach only after the same
            // authenticated handshake used by the normal path.
            if let Some(existing) = wait_for_owner(
                || attach_existing(&expected_data_dir),
                OWNER_CONVERGENCE_TIMEOUT,
            )? {
                return Ok(Attached {
                    runtime: existing,
                    _child: None,
                });
            }
            return fail_child(child, anyhow!("startup: child exited with {status}"));
        }
        match read_runtime_file(&path) {
            Ok(Some(runtime)) => {
                if previous
                    .as_ref()
                    .is_some_and(|old| same_identity(old, &runtime))
                {
                    last_observation = "runtime file still names the previous runtime".to_owned();
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                match verify_classified(&runtime, &expected_data_dir) {
                    Ok(runtime) => {
                        return Ok(Attached {
                            runtime,
                            _child: Some(child),
                        });
                    }
                    Err(VerifyError::Unauthenticated(error)) => {
                        last_observation = error.to_string();
                    }
                    Err(VerifyError::Incompatible(error)) => return fail_child(child, error),
                }
            }
            Ok(None) => {}
            Err(error) => {
                last_observation = error.to_string();
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    fail_child(
        child,
        anyhow!("startup: timed out after 20 seconds ({last_observation})"),
    )
}

fn read_runtime_file(path: &Path) -> Result<Option<Runtime>> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Runtime>(&text)
            .map(Some)
            .with_context(|| {
                format!(
                    "startup: runtime discovery file is malformed ({})",
                    path.display()
                )
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow!(
            "startup: cannot read runtime discovery file {}: {error}",
            path.display()
        )),
    }
}

fn fail_child(mut child: Child, error: anyhow::Error) -> Result<Attached> {
    let _ = child.kill();
    let _ = child.wait();
    Err(error)
}

fn wait_for_owner(
    mut discover: impl FnMut() -> Result<Option<Runtime>>,
    timeout: Duration,
) -> Result<Option<Runtime>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(runtime) = discover()? {
            return Ok(Some(runtime));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(None)
}

#[cfg(test)]
fn verify(runtime: &Runtime, expected_data_dir: &str) -> Result<Runtime> {
    verify_classified(runtime, expected_data_dir).map_err(VerifyError::into_error)
}

enum VerifyError {
    Unauthenticated(anyhow::Error),
    Incompatible(anyhow::Error),
}

#[cfg(test)]
impl VerifyError {
    fn into_error(self) -> anyhow::Error {
        match self {
            Self::Unauthenticated(error) => {
                if error.to_string().starts_with("unauthorized:") {
                    error
                } else {
                    anyhow!("startup: wrong listener: {error}")
                }
            }
            Self::Incompatible(error) => error,
        }
    }
}

fn verify_classified(runtime: &Runtime, expected_data_dir: &str) -> Result<Runtime, VerifyError> {
    if runtime.port == 0 {
        return Err(VerifyError::Unauthenticated(anyhow!(
            "startup: runtime advertised port 0"
        )));
    }
    if runtime.token.is_empty() {
        return Err(VerifyError::Unauthenticated(anyhow!(
            "startup: runtime advertised an empty token"
        )));
    }

    let health = health(runtime).map_err(VerifyError::Unauthenticated)?;
    if runtime.data_dir_fingerprint != expected_data_dir {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime data directory does not match this launch"
        )));
    }
    if health["runtime_id"] != runtime.runtime_id {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime identity mismatch"
        )));
    }
    if health["data_dir_fingerprint"] != runtime.data_dir_fingerprint {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime data directory identity mismatch"
        )));
    }
    if health["api_version"] != runtime.api_version {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime API identity mismatch"
        )));
    }
    if runtime.api_version != "v1" {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: incompatible API version {}",
            runtime.api_version
        )));
    }
    if health["build_provenance"] != runtime.build_provenance {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime build identity mismatch"
        )));
    }
    if runtime.build_provenance != format!("farseer-api/{}", env!("CARGO_PKG_VERSION")) {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: incompatible runtime build {}",
            runtime.build_provenance
        )));
    }
    let features = health["features"].as_array().ok_or_else(|| {
        VerifyError::Incompatible(anyhow!("startup: runtime did not report features"))
    })?;
    if runtime.features.len() != features.len()
        || runtime
            .features
            .iter()
            .any(|feature| !features.iter().any(|reported| reported == feature))
    {
        return Err(VerifyError::Incompatible(anyhow!(
            "startup: runtime feature identity mismatch"
        )));
    }
    for required in REQUIRED_FEATURES {
        if !features.iter().any(|feature| feature == required) {
            return Err(VerifyError::Incompatible(anyhow!(
                "startup: required feature `{required}` is unavailable"
            )));
        }
    }
    Ok(runtime.clone())
}

fn same_identity(left: &Runtime, right: &Runtime) -> bool {
    left.runtime_id == right.runtime_id && left.token == right.token
}

fn health(runtime: &Runtime) -> Result<serde_json::Value> {
    let address = (std::net::Ipv4Addr::LOCALHOST, runtime.port);
    let mut stream = std::net::TcpStream::connect_timeout(&address.into(), CONNECT_TIMEOUT)
        .context("loopback endpoint did not answer")?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(IO_TIMEOUT)))?;
    write!(
        stream,
        "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        runtime.port, runtime.token
    )?;
    stream.flush()?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let (headers, body) = response
        .split_once_bytes(b"\r\n\r\n")
        .ok_or_else(|| anyhow!("listener returned an invalid HTTP response"))?;
    let status = std::str::from_utf8(headers)
        .ok()
        .and_then(|headers| headers.lines().next())
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("listener returned no HTTP status"))?;
    if status == 401 || status == 403 {
        return Err(anyhow!(
            "unauthorized: runtime rejected the discovery token ({status})"
        ));
    }
    if status != 200 {
        return Err(anyhow!("listener returned HTTP {status}"));
    }
    serde_json::from_slice(body).context("health response was not JSON")
}

trait SplitOnceBytes {
    fn split_once_bytes(&self, needle: &[u8]) -> Option<(&[u8], &[u8])>;
}

impl SplitOnceBytes for [u8] {
    fn split_once_bytes(&self, needle: &[u8]) -> Option<(&[u8], &[u8])> {
        self.windows(needle.len())
            .position(|window| window == needle)
            .map(|at| (&self[..at], &self[at + needle.len()..]))
    }
}

/// Where the farseer binary is, next to this executable in an installed build
/// and in the same target directory during development.
pub fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "farseer.exe"
    } else {
        "farseer"
    });
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn runtime(port: u16) -> Runtime {
        Runtime {
            port,
            token: "token".into(),
            runtime_id: "runtime".into(),
            data_dir_fingerprint: "sha256:data".into(),
            api_version: "v1".into(),
            build_provenance: format!("farseer-api/{}", env!("CARGO_PKG_VERSION")),
            features: REQUIRED_FEATURES
                .iter()
                .map(|feature| (*feature).into())
                .collect(),
        }
    }

    fn listener(status: u16, body: serde_json::Value) -> u16 {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let _ = stream.read_to_end(&mut request);
            let bytes = body.to_string();
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{bytes}",
                bytes.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
        });
        port
    }

    fn health_body(runtime: &Runtime) -> serde_json::Value {
        serde_json::json!({
            "api_version": runtime.api_version,
            "runtime_id": runtime.runtime_id,
            "data_dir_fingerprint": runtime.data_dir_fingerprint,
            "build_provenance": runtime.build_provenance,
            "features": runtime.features,
        })
    }

    #[test]
    fn a_matching_health_handshake_is_accepted() {
        let mut runtime = runtime(0);
        let body = health_body(&runtime);
        runtime.port = listener(200, body);
        let result = verify(&runtime, "sha256:data");
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn a_wrong_listener_and_unauthorized_listener_are_distinct() {
        let mut runtime = runtime(0);
        runtime.port = listener(404, serde_json::json!({"answer": "other service"}));
        let wrong = verify(&runtime, "sha256:data").unwrap_err().to_string();
        assert!(wrong.contains("wrong listener"), "{wrong}");

        runtime.port = listener(401, serde_json::json!({}));
        let unauthorized = verify(&runtime, "sha256:data").unwrap_err().to_string();
        assert!(unauthorized.contains("unauthorized"), "{unauthorized}");
    }

    #[test]
    fn an_unreachable_listener_is_retryable_but_an_authenticated_mismatch_is_fatal() {
        let unreachable = runtime(9);
        assert!(matches!(
            verify_classified(&unreachable, "sha256:data"),
            Err(VerifyError::Unauthenticated(_))
        ));

        let mut runtime = runtime(0);
        let body = health_body(&runtime);
        runtime.port = listener(200, body);
        assert!(matches!(
            verify_classified(&runtime, "sha256:other"),
            Err(VerifyError::Incompatible(_))
        ));
    }

    #[test]
    fn identity_fingerprint_and_features_are_required() {
        let mut runtime = runtime(0);
        assert!(verify(&runtime, "sha256:other").is_err());
        runtime.port = 0;
        assert!(
            verify(&runtime, "sha256:data")
                .unwrap_err()
                .to_string()
                .contains("port 0")
        );

        runtime.port = listener(
            200,
            serde_json::json!({
                "api_version": "v1",
                "runtime_id": "runtime",
                "data_dir_fingerprint": "sha256:data",
                "build_provenance": format!("farseer-api/{}", env!("CARGO_PKG_VERSION")),
                "features": ["health"],
            }),
        );
        let error = verify(&runtime, "sha256:data").unwrap_err().to_string();
        assert!(error.contains("feature"), "{error}");
    }

    #[test]
    fn a_losing_launch_reuses_the_verified_owner_after_its_child_exits() {
        let expected = runtime(42);
        let mut attempts = 0;
        let found = wait_for_owner(
            || {
                attempts += 1;
                Ok((attempts >= 2).then_some(expected.clone()))
            },
            Duration::from_millis(250),
        )
        .unwrap();
        assert_eq!(found.unwrap().runtime_id, "runtime");
        assert!(attempts >= 2);
    }

    #[test]
    fn a_malformed_discovery_file_is_an_actionable_startup_error() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"not-json").unwrap();
        let error = read_runtime_file(file.path()).unwrap_err().to_string();
        assert!(
            error.contains("runtime discovery file is malformed"),
            "{error}"
        );
    }
}
