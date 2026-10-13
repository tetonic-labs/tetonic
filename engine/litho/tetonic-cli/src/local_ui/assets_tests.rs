use super::*;
use http_body_util::BodyExt;

fn bundle() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("assets")).unwrap();
    std::fs::write(root.path().join("index.html"), "<main>Tetonic</main>").unwrap();
    std::fs::write(root.path().join("assets/app-a.js"), "export const value=1;").unwrap();
    std::fs::write(root.path().join("assets/font.woff2"), [1, 2, 3]).unwrap();
    root
}

fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:3000".parse().unwrap());
    headers
}

#[tokio::test]
async fn serves_built_files_with_correct_types_and_no_credential_or_file_fallback() {
    let root = bundle();
    for name in [
        "connection.url",
        "workspace.db",
        "secret.json",
        "assets/app-a.js.map",
    ] {
        std::fs::write(root.path().join(name), "PRIVATECANARY").unwrap();
    }
    let assets = UiAssets::load(root.path()).unwrap();
    for (path, mime) in [
        ("/?engine=local", "text/html; charset=utf-8"),
        ("/assets/app-a.js", "text/javascript; charset=utf-8"),
        ("/assets/font.woff2", "font/woff2"),
    ] {
        let response = assets.serve(
            &Method::GET,
            &path.parse().unwrap(),
            &headers(),
            "127.0.0.1:3000",
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATECANARY"));
    }
    for path in [
        "/connection.url",
        "/workspace.db",
        "/secret.json",
        "/assets/app-a.js.map",
        "/assets/../index.html",
        "/assets/%2e%2e/index.html",
        "/missing.js",
        "/api/local/workspace",
    ] {
        let response = assets.serve(
            &Method::GET,
            &path.parse().unwrap(),
            &headers(),
            "127.0.0.1:3000",
        );
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn head_is_empty_and_assets_remain_a_startup_snapshot() {
    let root = bundle();
    let assets = UiAssets::load(root.path()).unwrap();
    std::fs::write(root.path().join("index.html"), "CHANGED").unwrap();
    let response = assets.serve(
        &Method::HEAD,
        &"/".parse().unwrap(),
        &headers(),
        "127.0.0.1:3000",
    );
    assert_eq!(response.headers()["content-length"], "20");
    assert!(response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
    let response = assets.serve(
        &Method::GET,
        &"/".parse().unwrap(),
        &headers(),
        "127.0.0.1:3000",
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "<main>Tetonic</main>"
    );
}

#[test]
fn rejects_wrong_host_cross_origin_and_mutation_requests() {
    let root = bundle();
    let assets = UiAssets::load(root.path()).unwrap();
    let mut incoming = headers();
    incoming.insert("host", "attacker.example".parse().unwrap());
    assert_eq!(
        assets
            .serve(
                &Method::GET,
                &"/".parse().unwrap(),
                &incoming,
                "127.0.0.1:3000"
            )
            .status(),
        StatusCode::FORBIDDEN
    );
    incoming = headers();
    incoming.insert("origin", "https://attacker.example".parse().unwrap());
    assert_eq!(
        assets
            .serve(
                &Method::GET,
                &"/".parse().unwrap(),
                &incoming,
                "127.0.0.1:3000"
            )
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        assets
            .serve(
                &Method::POST,
                &"/".parse().unwrap(),
                &headers(),
                "127.0.0.1:3000"
            )
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[test]
fn refuses_missing_entry_and_excessively_large_assets() {
    let root = tempfile::tempdir().unwrap();
    assert!(UiAssets::load(root.path()).is_err());
    std::fs::write(root.path().join("index.html"), "").unwrap();
    assert!(UiAssets::load(root.path()).is_err());
    let root = bundle();
    let large = std::fs::File::create(root.path().join("assets/large.js")).unwrap();
    large.set_len(MAX_ASSET_BYTES as u64 + 1).unwrap();
    assert!(UiAssets::load(root.path()).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_public_files() {
    let root = bundle();
    let private = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(private.path(), root.path().join("assets/escape.js")).unwrap();
    assert!(UiAssets::load(root.path()).is_err());
}
