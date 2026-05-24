//! `knogg update` — self-update from GitHub releases.
//!
//! Checks the latest published release, compares it with the running build's
//! version, and (when newer) downloads the binary matching the current OS/arch
//! and swaps it in place. Already on the latest version => no download.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde::Deserialize;

const REPO_OWNER: &str = "CoffeJeanCode";
const REPO_NAME: &str = "knogg";
const USER_AGENT: &str = concat!("knogg-updater/", env!("CARGO_PKG_VERSION"));

/// Min gap between passive network checks (24h) — keeps it non-invasive.
const CHECK_INTERVAL_SECS: u64 = 86_400;
/// Opt-out env var for the passive check.
const OPT_OUT_ENV: &str = "KNOGG_NO_UPDATE_CHECK";

#[derive(Debug, Deserialize)]
struct Release {
    /// Release tag, e.g. "v1.2.0".
    tag_name: String,
    /// Human page for the release notes.
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Release asset name for the OS/arch this binary was built for.
///
/// Names mirror `.github/workflows/release.yml`. `None` => unsupported
/// platform (no prebuilt binary published), so self-update is impossible.
fn target_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("knogg-linux-amd64"),
        ("windows", "x86_64") => Some("knogg-windows-amd64.exe"),
        ("macos", "x86_64") => Some("knogg-macos-amd64"),
        ("macos", "aarch64") => Some("knogg-macos-arm64"),
        _ => None,
    }
}

/// Parse a release tag ("v1.2.0" or "1.2.0") into a semver `Version`.
fn parse_tag(tag: &str) -> Result<Version> {
    let raw = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(raw).with_context(|| format!("unparseable release tag '{tag}'"))
}

/// Fetch the latest release metadata from the GitHub API.
///
/// `timeout` bounds the whole request — used short for the passive check so it
/// never stalls a normal command.
fn latest_release(timeout: Option<Duration>) -> Result<Release> {
    let url =
        format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let mut req = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json");
    if let Some(t) = timeout {
        req = req.timeout(t);
    }
    let resp = req
        .call()
        .with_context(|| format!("GitHub API request failed ({url})"))?;
    resp.into_json::<Release>()
        .context("could not parse GitHub release response")
}

/// `knogg update`: check for a newer release and (unless `check_only`) install it.
pub fn run(check_only: bool) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("invalid built-in version")?;

    let asset_name = target_asset().ok_or_else(|| {
        anyhow!(
            "no prebuilt binary for this platform ({}/{}); build from source",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    println!("knogg update — current version v{current}");
    println!("checking github.com/{REPO_OWNER}/{REPO_NAME} for newer releases...");

    let release = latest_release(Some(Duration::from_secs(15)))?;
    let latest = parse_tag(&release.tag_name)?;
    write_cache(&release.tag_name); // refresh passive cache too

    if latest <= current {
        println!("Already on the latest version (v{current}). Nothing to do.");
        return Ok(());
    }

    // New version notice.
    println!();
    println!("==> A new version of knogg is available: v{current} -> v{latest}");
    println!("    Release notes: {}", release.html_url);

    if check_only {
        println!("    Run `knogg update` to install it.");
        return Ok(());
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            anyhow!("release v{latest} has no asset '{asset_name}' for this platform")
        })?;

    println!("downloading {} ...", asset.name);
    let bytes = download(&asset.browser_download_url)?;

    install(&bytes).context("failed to replace the running binary")?;

    println!("Updated to v{latest}. Restart knogg to use the new version.");
    Ok(())
}

/// Download a release asset into memory (follows GitHub's CDN redirect).
fn download(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("download failed ({url})"))?;

    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .context("error while reading download stream")?;
    if buf.is_empty() {
        bail!("downloaded asset was empty");
    }
    Ok(buf)
}

/// Write the new binary to a temp file and atomically swap it for the running
/// executable (handles the running-process case on every OS via `self-replace`).
fn install(bytes: &[u8]) -> Result<()> {
    let exe = std::env::current_exe().context("cannot locate current executable")?;
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));

    // Stage in the destination dir so the final swap is a same-filesystem rename.
    let tmp = dir.join(format!(".knogg-update-{}", std::process::id()));
    std::fs::write(&tmp, bytes)
        .with_context(|| format!("cannot write temp file {}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .context("cannot set executable permissions")?;
    }

    let res = self_replace::self_replace(&tmp);
    let _ = std::fs::remove_file(&tmp);
    res.context("self-replace failed")
}

// ---------------------------------------------------------------------------
// Passive check — a quiet, cached, rate-limited reminder.
// ---------------------------------------------------------------------------

/// Cache file holding the last passive check: `<unix_secs> <tag>`.
fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".knogg").join("update_check"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read `(checked_at, tag)` from the cache, if present and well-formed.
fn read_cache() -> Option<(u64, String)> {
    let raw = std::fs::read_to_string(cache_path()?).ok()?;
    let (ts, tag) = raw.trim().split_once(' ')?;
    Some((ts.parse().ok()?, tag.to_string()))
}

/// Persist the latest seen tag with the current timestamp (best-effort).
fn write_cache(tag: &str) {
    if let Some(p) = cache_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, format!("{} {tag}", now_secs()));
    }
}

/// Best-effort, non-invasive "new version" notice for normal commands.
///
/// Rules that keep it polite:
/// - silent on every error (never blocks or fails the real command);
/// - hits the network at most once per [`CHECK_INTERVAL_SECS`] (cached otherwise);
/// - 3s network timeout so a slow GitHub never delays you;
/// - prints a single line to **stderr** (won't pollute piped stdout);
/// - opt out entirely with `KNOGG_NO_UPDATE_CHECK=1`.
pub fn passive_notify() {
    if std::env::var_os(OPT_OUT_ENV).is_some() {
        return;
    }
    if target_asset().is_none() {
        return; // no upgrade path on this platform — stay quiet
    }
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return;
    };

    let cached = read_cache();
    let fresh = cached
        .as_ref()
        .is_some_and(|(ts, _)| now_secs().saturating_sub(*ts) < CHECK_INTERVAL_SECS);

    // Use the cached tag while fresh; otherwise probe the network quickly.
    let tag = if fresh {
        cached.map(|(_, tag)| tag)
    } else {
        match latest_release(Some(Duration::from_secs(3))) {
            Ok(rel) => {
                write_cache(&rel.tag_name);
                Some(rel.tag_name)
            }
            // Network failed: fall back to a stale cached tag if we have one.
            Err(_) => cached.map(|(_, tag)| tag),
        }
    };

    let Some(tag) = tag else { return };
    let Ok(latest) = parse_tag(&tag) else { return };

    if latest > current {
        eprintln!(
            "==> knogg v{latest} is available (you have v{current}). Run `knogg update`. \
             Silence with {OPT_OUT_ENV}=1."
        );
    }
}
