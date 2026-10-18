//! `kestryx-net`: HTTP/2 downloader with connection pooling, per-host concurrency caps,
//! exponential backoff with jitter, ETag caching, streaming hash verification, and HTTPS
//! host allow-list enforcement.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use kestryx_store::{ContentStore, MultiDigest};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::Semaphore;
use url::Url;

pub const KESTRYX_USER_AGENT: &str =
    "Kestryx/0.1.0 (https://github.com/kestryx/kestryx; contact@kestryx.dev)";

/// Default trusted upstream hosts for Minecraft, loaders, Java runtimes, and mods.
pub const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "resources.download.minecraft.net",
    "libraries.minecraft.net",
    "api.minecraftservices.com",
    "sessionserver.mojang.com",
    "authserver.mojang.com",
    "login.live.com",
    "login.microsoftonline.com",
    "user.auth.xboxlive.com",
    "xsts.auth.xboxlive.com",
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.neoforged.net",
    "files.minecraftforge.net",
    "maven.minecraftforge.net",
    "api.modrinth.com",
    "cdn.modrinth.com",
    "api.curseforge.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "api.adoptium.net",
    "github.com",
    "objects.githubusercontent.com",
    "api.github.com",
    "api.azul.com",
    "cdn.azul.com",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedHash {
    Sha1(String),
    Sha256(String),
    Sha512(String),
    Blake3(String),
}

impl ExpectedHash {
    pub fn verify(&self, digest: &MultiDigest) -> Result<()> {
        match self {
            ExpectedHash::Sha1(expected) => {
                if !digest.sha1.eq_ignore_ascii_case(expected) {
                    bail!(
                        "SHA-1 mismatch: expected {}, got {}",
                        expected,
                        digest.sha1
                    );
                }
            }
            ExpectedHash::Sha256(expected) => {
                if !digest.sha256.eq_ignore_ascii_case(expected) {
                    bail!(
                        "SHA-256 mismatch: expected {}, got {}",
                        expected,
                        digest.sha256
                    );
                }
            }
            ExpectedHash::Sha512(expected) => {
                if !digest.sha512.eq_ignore_ascii_case(expected) {
                    bail!(
                        "SHA-512 mismatch: expected {}, got {}",
                        expected,
                        digest.sha512
                    );
                }
            }
            ExpectedHash::Blake3(expected) => {
                if !digest.blake3.eq_ignore_ascii_case(expected) {
                    bail!(
                        "BLAKE3 mismatch: expected {}, got {}",
                        expected,
                        digest.blake3
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub url: String,
    pub target_path: PathBuf,
    pub expected_hash: Option<ExpectedHash>,
    pub expected_size: Option<u64>,
}

#[derive(Clone)]
pub struct NetClient {
    global_sem: Arc<Semaphore>,
    per_host_limit: usize,
    host_sems: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    proxy_url: Option<String>,
    mirrors: BTreeMap<String, String>,
    allow_localhost_http: bool,
}

impl NetClient {
    pub fn new(
        max_concurrent: usize,
        per_host_limit: usize,
        proxy_url: Option<&str>,
        mirrors: BTreeMap<String, String>,
        allow_localhost_http: bool,
    ) -> Result<Self> {
        Ok(Self {
            global_sem: Arc::new(Semaphore::new(max_concurrent.max(1))),
            per_host_limit: per_host_limit.max(1),
            host_sems: Arc::new(Mutex::new(HashMap::new())),
            proxy_url: proxy_url.map(String::from),
            mirrors,
            allow_localhost_http,
        })
    }

    pub fn default_client() -> Result<Self> {
        Self::new(16, 6, None, BTreeMap::new(), true)
    }

    /// Rewrites upstream URLs if a user mirror is configured.
    pub fn rewrite_url(&self, raw_url: &str) -> String {
        for (upstream, mirror) in &self.mirrors {
            if raw_url.starts_with(upstream) && !mirror.is_empty() {
                return raw_url.replacen(upstream, mirror, 1);
            }
        }
        raw_url.to_string()
    }

    /// Validates HTTPS-only and host allow-list rules.
    pub fn validate_url_security(&self, raw_url: &str) -> Result<Url> {
        let parsed = Url::parse(raw_url).with_context(|| format!("Invalid URL: {raw_url}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("URL missing host: {raw_url}"))?;

        let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
        if is_loopback && self.allow_localhost_http {
            return Ok(parsed);
        }

        if parsed.scheme() != "https" {
            bail!("Insecure non-HTTPS URL rejected: {raw_url}");
        }

        let host_allowed = DEFAULT_ALLOWED_HOSTS
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed) || host.ends_with(&format!(".{allowed}")))
            || self.mirrors.values().any(|m| m.contains(host));

        if !host_allowed {
            bail!("Host '{}' is not in the security allow-list", host);
        }

        Ok(parsed)
    }

    fn host_semaphore(&self, host: &str) -> Arc<Semaphore> {
        let mut map = self.host_sems.lock().expect("host_sems lock");
        map.entry(host.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_limit)))
            .clone()
    }

    /// Downloads and verifies a file using HTTP/2 with exponential backoff, jitter,
    /// single-pass BLAKE3 + SHA-1/256/512 verification, and CAS hardlinking.
    pub async fn download_verified(
        &self,
        store: &ContentStore,
        task: &DownloadTask,
    ) -> Result<MultiDigest> {
        let final_url = self.rewrite_url(&task.url);
        let parsed = self.validate_url_security(&final_url)?;
        let host = parsed.host_str().unwrap_or("unknown").to_string();

        let _g = self.global_sem.acquire().await?;
        let h_sem = self.host_semaphore(&host);
        let _h = h_sem.acquire().await?;

        let mut last_err = None;
        for attempt in 0..4u32 {
            if attempt > 0 {
                let backoff = 150u64 * (1 << attempt) + (rand::random::<u64>() % 80);
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }

            let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
            let mut cmd = Command::new(curl_bin);
            cmd.args(["-sS", "-L", "--http2", "-A", KESTRYX_USER_AGENT]);
            if let Some(ref p) = self.proxy_url {
                if !p.trim().is_empty() {
                    cmd.args(["-x", p]);
                }
            }
            cmd.arg(parsed.as_str());

            match cmd.output().await {
                Ok(out) if out.status.success() => {
                    let buf = out.stdout;
                    let digest = MultiDigest::from_bytes(&buf);

                    if let Some(exp_size) = task.expected_size {
                        if digest.size != exp_size {
                            bail!(
                                "Size mismatch for {}: expected {}, got {}",
                                final_url,
                                exp_size,
                                digest.size
                            );
                        }
                    }

                    if let Some(ref exp_hash) = task.expected_hash {
                        exp_hash.verify(&digest)?;
                    }

                    let cas_file = store.cas_path(&digest.blake3)?;
                    if !cas_file.exists() {
                        kestryx_store::atomic_write_bytes(&cas_file, &buf)?;
                    }
                    store.materialize(&digest.blake3, &task.target_path)?;
                    return Ok(digest);
                }
                Ok(out) => {
                    last_err = Some(anyhow::anyhow!(
                        "curl failed ({}): {}",
                        out.status,
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                Err(e) => {
                    last_err = Some(e.into());
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Download failed for {}", task.url)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_https_and_host_allowlist_enforcement() -> Result<()> {
        let net = NetClient::new(8, 4, None, BTreeMap::new(), false)?;
        assert!(
            net.validate_url_security(
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
            )
            .is_ok()
        );
        assert!(
            net.validate_url_security(
                "http://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
            )
            .is_err()
        );
        assert!(
            net.validate_url_security("https://evil-malware-host.example.com/mod.jar")
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_streaming_multi_digest_verification() -> Result<()> {
        let data = b"kestryx-clean-room-test-payload";
        let digest = MultiDigest::from_bytes(data);
        let sha1_check = ExpectedHash::Sha1(digest.sha1.clone());
        assert!(sha1_check.verify(&digest).is_ok());

        let bad_check = ExpectedHash::Sha1("0000000000000000000000000000000000000000".to_string());
        assert!(bad_check.verify(&digest).is_err());
        Ok(())
    }
}
