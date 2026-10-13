//! Public product files, snapshotted at startup separately from all engine state.
use super::{Bytes, Full, Response, StatusCode};
use hyper::{HeaderMap, Method, Uri};
use std::{collections::HashMap, io::Read, path::Path};

const MAX_ASSET_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_ASSETS: usize = 4096;

pub(super) struct UiAssets {
    files: HashMap<String, (&'static str, Bytes)>,
}

impl UiAssets {
    pub(super) fn load(root: &Path) -> anyhow::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        anyhow::ensure!(
            root.is_dir(),
            "--ui-dir must name a built product UI directory"
        );
        let mut files = HashMap::new();
        let mut pending = vec![(root.clone(), String::new())];
        let mut total = 0usize;
        let mut entries = 0usize;
        while let Some((directory, prefix)) = pending.pop() {
            anyhow::ensure!(
                prefix.matches('/').count() <= 16,
                "UI asset paths are too deep"
            );
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                entries += 1;
                anyhow::ensure!(entries <= MAX_ASSETS, "Too many UI bundle entries");
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !safe_segment(&name) {
                    continue;
                }
                let route = format!("{prefix}/{name}");
                let kind = entry.file_type()?;
                if route == "/assets" || content_type(&route).is_some() || kind.is_dir() {
                    anyhow::ensure!(
                        !is_link(&entry.metadata()?),
                        "UI assets must not be symbolic links or junctions"
                    );
                }
                if kind.is_dir() && (route == "/assets" || route.starts_with("/assets/")) {
                    pending.push((entry.path(), route));
                    anyhow::ensure!(pending.len() <= MAX_ASSETS, "Too many UI directories");
                    continue;
                }
                let Some(mime) = content_type(&route) else {
                    continue;
                };
                anyhow::ensure!(!kind.is_symlink(), "UI assets must not be symbolic links");
                if !kind.is_file() {
                    continue;
                }
                anyhow::ensure!(
                    entry.metadata()?.len() <= MAX_ASSET_BYTES as u64,
                    "UI asset is too large"
                );
                let mut bytes = Vec::new();
                std::fs::File::open(entry.path())?
                    .take(MAX_ASSET_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                anyhow::ensure!(bytes.len() <= MAX_ASSET_BYTES, "UI asset is too large");
                total = total
                    .checked_add(bytes.len())
                    .ok_or_else(|| anyhow::anyhow!("UI assets are too large"))?;
                anyhow::ensure!(
                    total <= MAX_TOTAL_BYTES && files.len() < MAX_ASSETS,
                    "UI bundle exceeds its size limit"
                );
                files.insert(route, (mime, Bytes::from(bytes)));
            }
        }
        anyhow::ensure!(files.get("/index.html").is_some_and(|(_, bytes)| !bytes.is_empty()),
            "Built UI index.html is missing. Build the web product and pass its dist directory to --ui-dir.");
        Ok(Self { files })
    }

    pub(super) fn serve(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
        host: &str,
    ) -> Response<Full<Bytes>> {
        if headers.get("host").and_then(|v| v.to_str().ok()) != Some(host)
            || headers
                .get("origin")
                .is_some_and(|v| v.to_str().ok() != Some(format!("http://{host}").as_str()))
        {
            return response(
                StatusCode::FORBIDDEN,
                "text/plain; charset=utf-8",
                Bytes::new(),
                false,
            );
        }
        if method != Method::GET && method != Method::HEAD {
            let mut result = response(
                StatusCode::METHOD_NOT_ALLOWED,
                "text/plain; charset=utf-8",
                Bytes::new(),
                false,
            );
            result
                .headers_mut()
                .insert("allow", "GET, HEAD".parse().unwrap());
            return result;
        }
        let route = if uri.path() == "/" {
            "/index.html"
        } else {
            uri.path()
        };
        match self.files.get(route) {
            Some((mime, bytes)) => {
                response(StatusCode::OK, mime, bytes.clone(), method == Method::HEAD)
            }
            None => response(
                StatusCode::NOT_FOUND,
                "text/plain; charset=utf-8",
                Bytes::new(),
                false,
            ),
        }
    }
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn safe_segment(name: &str) -> bool {
    !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn content_type(route: &str) -> Option<&'static str> {
    if route == "/index.html" {
        return Some("text/html; charset=utf-8");
    }
    if route != "/favicon.svg" && route != "/tetonic-symbol.png" && !route.starts_with("/assets/") {
        return None;
    }
    match route.rsplit('.').next()? {
        "js" => Some("text/javascript; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        "ico" => Some("image/x-icon"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        _ => None,
    }
}

fn response(status: StatusCode, mime: &str, bytes: Bytes, head: bool) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", mime)
        .header("content-length", bytes.len())
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .header("x-frame-options", "DENY")
        .header("cross-origin-resource-policy", "same-origin")
        .body(Full::new(if head { Bytes::new() } else { bytes }))
        .unwrap()
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
