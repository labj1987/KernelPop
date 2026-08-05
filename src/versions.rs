//! versions.rs — Talk to kernel.ubuntu.com/mainline: list versions, resolve
//! the generic-flavour amd64 .deb set for a version, and fetch checksums.

use anyhow::{bail, Context, Result};
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashMap;

pub const MAINLINE_BASE: &str = "https://kernel.ubuntu.com/mainline/";

#[derive(Debug, Clone)]
pub struct KernelVersion {
    /// e.g. "7.1.3"
    pub version: String,
    /// e.g. "https://kernel.ubuntu.com/mainline/v7.1.3/"
    pub url: String,
}

/// One .deb belonging to a kernel version.
#[derive(Debug, Clone)]
pub struct KernelDeb {
    pub filename: String,
    pub url: String,
    /// SHA256 from the CHECKSUMS file, if published.
    pub sha256: Option<String>,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("kernelpop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?)
}

/// List stable versions from the mainline index, newest first.
/// Release candidates and the daily builds are skipped.
pub async fn fetch_versions() -> Result<Vec<KernelVersion>> {
    let html = client()?
        .get(MAINLINE_BASE)
        .send()
        .await
        .context("Failed to reach kernel.ubuntu.com")?
        .text()
        .await?;

    let document = Html::parse_document(&html);
    let selector = Selector::parse("a[href]").unwrap();
    // Directories look like "v7.1.3/"; skip v7.2-rc1/, daily/, etc.
    let ver_re = Regex::new(r"^v(\d+\.\d+(?:\.\d+)?)/$").unwrap();

    let mut versions: Vec<KernelVersion> = document
        .select(&selector)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            let caps = ver_re.captures(href)?;
            let version = caps[1].to_string();
            let url = format!("{}v{}/", MAINLINE_BASE, version);
            Some(KernelVersion { version, url })
        })
        .collect();

    versions.sort_by(|a, b| {
        let av: Vec<u32> = a.version.split('.').filter_map(|s| s.parse().ok()).collect();
        let bv: Vec<u32> = b.version.split('.').filter_map(|s| s.parse().ok()).collect();
        bv.cmp(&av)
    });
    versions.dedup_by(|a, b| a.version == b.version);

    if versions.is_empty() {
        bail!("No kernel versions found in the mainline index");
    }
    Ok(versions)
}

/// Collect .deb links from one index page. Hrefs may be plain filenames or
/// prefixed with a subdirectory (e.g. "amd64/linux-image-…"), so URLs are
/// resolved against the page they came from.
fn debs_from_page(page_url: &str, html: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a[href]").unwrap();
    let mut out = vec![];
    for el in document.select(&selector) {
        let Some(href) = el.value().attr("href") else { continue };
        if !href.ends_with(".deb") || href.starts_with("..") {
            continue;
        }
        let filename = href.rsplit('/').next().unwrap_or(href).to_string();
        let url = format!("{}{}", page_url, href.trim_start_matches("./"));
        out.push((filename, url));
    }
    out
}

/// True for the four generic-flavour amd64 packages this app installs:
/// linux-headers-*_all.deb, linux-headers-*-generic_*_amd64.deb,
/// linux-image-unsigned-*-generic_*_amd64.deb, linux-modules-*-generic_*_amd64.deb.
fn wanted_deb(filename: &str) -> bool {
    let generic_amd64 = filename.contains("-generic") && filename.ends_with("_amd64.deb");
    let headers_all = filename.starts_with("linux-headers-") && filename.ends_with("_all.deb");
    if headers_all {
        return true;
    }
    if !generic_amd64 {
        return false;
    }
    filename.starts_with("linux-image-unsigned-")
        || filename.starts_with("linux-image-")
        || filename.starts_with("linux-modules-")
        || filename.starts_with("linux-headers-")
}

/// Parse a mainline CHECKSUMS file: plain `hash  path` lines from sha1sum
/// and sha256sum runs. Only 64-char (SHA256) hashes are kept, keyed by the
/// path's basename so both flat and amd64/-prefixed layouts match.
fn parse_checksums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(path)) = (parts.next(), parts.next()) else { continue };
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let base = path.trim_start_matches('*').rsplit('/').next().unwrap_or(path);
        map.insert(base.to_string(), hash.to_lowercase());
    }
    map
}

/// Resolve the .deb set for one version. Tries the version page first
/// (older flat layout), then the amd64/ subdirectory (newer layout).
pub async fn fetch_deb_list(ver: &KernelVersion) -> Result<Vec<KernelDeb>> {
    let client = client()?;

    let top_html = client
        .get(&ver.url)
        .send()
        .await
        .context("Failed to fetch kernel version page")?
        .text()
        .await?;

    let mut found = debs_from_page(&ver.url, &top_html);

    if !found.iter().any(|(f, _)| wanted_deb(f)) {
        let amd64_url = format!("{}amd64/", ver.url);
        if let Ok(resp) = client.get(&amd64_url).send().await {
            if resp.status().is_success() {
                let html = resp.text().await?;
                found.extend(debs_from_page(&amd64_url, &html));
            }
        }
    }

    // Checksums are optional — a missing CHECKSUMS file is not fatal.
    let mut sums = HashMap::new();
    let checksums_url = format!("{}CHECKSUMS", ver.url);
    if let Ok(resp) = client.get(&checksums_url).send().await {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                sums = parse_checksums(&text);
            }
        }
    }

    let mut debs: Vec<KernelDeb> = vec![];
    for (filename, url) in found {
        if !wanted_deb(&filename) {
            continue;
        }
        if debs.iter().any(|d: &KernelDeb| d.filename == filename) {
            continue;
        }
        let sha256 = sums.get(&filename).cloned();
        debs.push(KernelDeb { filename, url, sha256 });
    }

    if debs.is_empty() {
        bail!(
            "No generic amd64 packages found for v{} — the build may have \
             failed for this architecture (check {} in a browser)",
            ver.version, ver.url
        );
    }
    debs.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(debs)
}
