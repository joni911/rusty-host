//! Version resolution (PRD §4.4).
//!
//! Turns a descriptor plus an optional version request into a concrete
//! downloadable release. Network access is abstracted behind [`Upstream`] so
//! every branch can be tested against recorded fixtures with no network.

use std::fmt;

use regex::Regex;

use crate::component::{ComponentKind, Descriptor, ResolveStrategy};

/// A concrete release we can download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Normalised `x.y.z` version.
    pub version: String,
    /// Absolute download URL.
    pub url: String,
    /// Expected digest, when upstream publishes one.
    pub sha256: Option<String>,
}

/// Fetch layer. Implemented by the real HTTP client and by test doubles.
pub trait Upstream {
    /// Fetch a URL as text.
    fn get(&self, url: &str) -> Result<String, ResolveError>;
}

/// Why resolution failed.
#[derive(Debug)]
pub enum ResolveError {
    /// Could not fetch or parse the upstream index.
    Upstream { url: String, detail: String },
    /// The index parsed but contained nothing usable.
    NoCandidates { component: String },
    /// The requested version does not exist upstream.
    NotFound { requested: String, available: Vec<String> },
    /// The descriptor itself is not actionable.
    BadDescriptor { detail: String },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Upstream { url, detail } => {
                write!(f, "could not read version index {url}: {detail}")
            }
            ResolveError::NoCandidates { component } => write!(
                f,
                "no installable versions found for `{component}`; the upstream index format may have changed"
            ),
            ResolveError::NotFound {
                requested,
                available,
            } => {
                let mut list = available.clone();
                list.sort();
                let shown: Vec<&str> = list.iter().take(8).map(String::as_str).collect();
                write!(
                    f,
                    "version `{requested}` is not available. Available: {}",
                    if shown.is_empty() {
                        "(none)".to_string()
                    } else {
                        shown.join(", ")
                    }
                )
            }
            ResolveError::BadDescriptor { detail } => write!(f, "invalid component descriptor: {detail}"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve `want` (e.g. `"8.4"`, `"8.4.27"`, or `None` for newest) for `comp`.
pub fn resolve(
    comp: &Descriptor,
    want: Option<&str>,
    upstream: &dyn Upstream,
) -> Result<Release, ResolveError> {
    match &comp.resolve {
        ResolveStrategy::Direct { url } => Ok(Release {
            version: version_from_url(url).unwrap_or_else(|| "latest".to_string()),
            url: url.clone(),
            sha256: None,
        }),
        ResolveStrategy::JsonIndex {
            index_url,
            url_field,
            base_url,
            variant,
        } => {
            let body = upstream.get(index_url)?;
            let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
                ResolveError::Upstream {
                    url: index_url.clone(),
                    detail: e.to_string(),
                }
            })?;

            let mut candidates = parse_json_index(&json, url_field.as_deref(), base_url.as_deref(), variant.as_deref());
            if candidates.is_empty() {
                return Err(ResolveError::NoCandidates {
                    component: comp.name.clone(),
                });
            }
            // PHP's index is keyed by minor ("8.4"); prefer the requested major.minor.
            candidates.sort_by(|a, b| version_cmp(&a.version, &b.version));
            pick(candidates, want, comp)
        }
        ResolveStrategy::HtmlScrape {
            index_url,
            file_regex,
            base_url,
            prefer_lts,
        } => {
            let body = upstream.get(index_url)?;
            let re = Regex::new(file_regex).map_err(|e| ResolveError::BadDescriptor {
                detail: format!("file_regex: {e}"),
            })?;

            let mut candidates: Vec<Release> = Vec::new();
            for caps in re.captures_iter(&body) {
                let Some(ver) = caps.get(1) else { continue };
                let version = normalise(ver.as_str());
                // The whole match is the file name; that gives us the URL.
                let file = caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_string();
                let url = match base_url {
                    Some(tpl) if tpl.contains("{version}") => {
                        tpl.replace("{version}", &version)
                    }
                    Some(base) if tpl_ends_with_file(base) => base.clone(),
                    _ => format!("{index_url}{file}"),
                };
                if !candidates.iter().any(|c| c.version == version) {
                    candidates.push(Release {
                        version,
                        url,
                        sha256: None,
                    });
                }
            }

            if candidates.is_empty() {
                return Err(ResolveError::NoCandidates {
                    component: comp.name.clone(),
                });
            }
            candidates.sort_by(|a, b| version_cmp(&a.version, &b.version));

            if want.is_none() && *prefer_lts {
                // Prefer the newest version whose major is a known LTS line.
                let lts: Vec<String> = candidates
                    .iter()
                    .filter(|c| is_lts(&c.version, comp.kind))
                    .map(|c| c.version.clone())
                    .collect();
                if let Some(best) = lts.last() {
                    let chosen = candidates
                        .iter()
                        .find(|c| &c.version == best)
                        .cloned()
                        .unwrap();
                    return Ok(chosen);
                }
            }
            pick(candidates, want, comp)
        }
        ResolveStrategy::GithubRelease {
            repo,
            asset_regex,
        } => {
            let api = format!("https://api.github.com/repos/{repo}/releases/latest");
            let body = upstream.get(&api)?;
            let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
                ResolveError::Upstream {
                    url: api.clone(),
                    detail: e.to_string(),
                }
            })?;

            let tag = json
                .get("tag_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let version = normalise(tag);

            let re = Regex::new(asset_regex).map_err(|e| ResolveError::BadDescriptor {
                detail: format!("asset_regex: {e}"),
            })?;

            let assets = json
                .get("assets")
                .and_then(|v| v.as_array())
                .ok_or_else(|| ResolveError::Upstream {
                    url: api.clone(),
                    detail: "no `assets` array".into(),
                })?;

            for a in assets {
                let Some(name) = a.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                if re.is_match(name) {
                    let url = a
                        .get("browser_download_url")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ResolveError::Upstream {
                            url: api.clone(),
                            detail: format!("asset `{name}` has no download URL"),
                        })?
                        .to_string();
                    return Ok(Release {
                        version,
                        url,
                        sha256: None,
                    });
                }
            }
            Err(ResolveError::NoCandidates {
                component: comp.name.clone(),
            })
        }
    }
}

fn pick(
    mut candidates: Vec<Release>,
    want: Option<&str>,
    comp: &Descriptor,
) -> Result<Release, ResolveError> {
    candidates.sort_by(|a, b| version_cmp(&a.version, &b.version));
    let available: Vec<String> = candidates.iter().map(|c| c.version.clone()).collect();

    let Some(want) = want else {
        return candidates
            .pop()
            .ok_or_else(|| ResolveError::NoCandidates {
                component: comp.name.clone(),
            });
    };
    let want = normalise(want);

    // Exact match wins.
    if let Some(exact) = candidates.iter().find(|c| c.version == want) {
        return Ok(exact.clone());
    }
    // Then newest matching prefix: "8.4" -> 8.4.27, and "8" -> 8.5.12.
    let prefix = format!("{want}.");
    let matching: Vec<&Release> = candidates
        .iter()
        .filter(|c| c.version == want || c.version.starts_with(&prefix))
        .collect();
    if let Some(best) = matching.last() {
        return Ok((*best).clone());
    }
    Err(ResolveError::NotFound {
        requested: want,
        available,
    })
}

/// Parse php.net's `releases.json`: `{ "8.4": { version, "<variant>": { zip: { path } } } }`
fn parse_json_index(
    json: &serde_json::Value,
    url_field: Option<&str>,
    base_url: Option<&str>,
    variant: Option<&str>,
) -> Vec<Release> {
    let mut out = Vec::new();
    let Some(obj) = json.as_object() else {
        return out;
    };

    for (_minor, entry) in obj {
        let raw_version = entry
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if raw_version.is_empty() {
            continue;
        }
        let version = normalise(raw_version);

        // Locate the download path for the wanted variant, else any variant
        // that has one (so a major that only ships vs16 still resolves).
        let mut path = None;
        if let Some(v) = variant {
            path = dig(entry, v, url_field);
        }
        if path.is_none() {
            if let Some(sub) = entry.as_object() {
                // Deterministic preference when the requested variant is absent:
                // thread-safe x64 first, then any thread-safe, then anything.
                let mut keys: Vec<&String> =
                    sub.keys().filter(|k| k.as_str() != "version").collect();
                keys.sort_by_key(|k| variant_rank(k));
                for k in keys {
                    if let Some(p) = dig(entry, k, url_field) {
                        path = Some(p);
                        break;
                    }
                }
            }
        }

        let Some(path) = path else { continue };
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path
        } else {
            format!("{}{}", base_url.unwrap_or_default(), path)
        };

        out.push(Release {
            version,
            url,
            sha256: None,
        });
    }
    out
}

/// Follow a dotted field path (`"zip.path"`) inside a variant object.
fn dig(entry: &serde_json::Value, variant: &str, url_field: Option<&str>) -> Option<String> {
    let mut node = entry.get(variant)?;
    let field = url_field.unwrap_or("path");
    for part in field.split('.') {
        node = node.get(part)?;
    }
    node.as_str().map(|s| s.to_string())
}

/// Rank alternative build variants when the preferred one is unavailable.
/// Lower is better: thread-safe x64 builds are what mod_php and most Windows
/// setups need, so NTS must never win a tie-break.
fn variant_rank(key: &str) -> u8 {
    let k = key.to_ascii_lowercase();
    let ts = k.starts_with("ts-") || k.contains("-ts-");
    let nts = k.starts_with("nts-") || k.contains("-nts-");
    let x64 = k.contains("x64");
    match (ts, nts, x64) {
        (true, _, true) => 0,
        (true, _, false) => 1,
        (false, false, true) => 2,
        (false, false, false) => 3,
        (false, true, true) => 4,
        (false, true, false) => 5,
    }
}

/// `php-8.4.27-1-Win32-vs17-x64.zip` -> `8.4.27`
fn normalise(v: &str) -> String {
    let trimmed = v.trim_start_matches('v');
    // Take the leading numeric dotted run.
    let mut out = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            out.push(ch);
        } else {
            break;
        }
    }
    while out.ends_with('.') {
        out.pop();
    }
    out
}

fn version_from_url(url: &str) -> Option<String> {
    let re = Regex::new(r"(\d+\.\d+\.\d+)").ok()?;
    re.captures(url).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

/// Compare dotted versions numerically, segment by segment.
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// MariaDB long-term-support major lines. Everything else is refreshed more
/// often and should not be a default.
fn is_lts(version: &str, kind: ComponentKind) -> bool {
    if kind != ComponentKind::Database {
        return false;
    }
    let mut parts = version.split('.');
    let major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    matches!((major, minor), (10, 6) | (10, 11) | (11, 4) | (11, 8))
}

fn tpl_ends_with_file(tpl: &str) -> bool {
    tpl.ends_with(".zip") || tpl.ends_with(".tar.gz") || tpl.ends_with(".7z")
}

