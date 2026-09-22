//! S-1(a)(b): cargo metadata resolve graph, not Cargo.toml silence.
//! Lives under tests/ so ARCH-PROC-001 does not treat `Command::new("cargo")` as a process mutant.

fn package_depends_on_index_or_lsp(pkg_name: &str) -> Result<Vec<String>, String> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let host = {
        let out = std::process::Command::new("rustc")
            .args(["-vV"])
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find_map(|l| l.strip_prefix("host: "))
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "rustc -vV missing host".to_string())?
    };
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--filter-platform",
            &host,
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let resolve = meta
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| "missing resolve.nodes".to_string())?;
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "missing packages".to_string())?;
    let id_to_name: std::collections::HashMap<&str, &str> = packages
        .iter()
        .filter_map(|p| {
            let id = p.get("id")?.as_str()?;
            let name = p.get("name")?.as_str()?;
            Some((id, name))
        })
        .collect();
    let mut name_to_id: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (id, name) in &id_to_name {
        name_to_id.insert(*name, *id);
    }
    let pkg_id = *name_to_id
        .get(pkg_name)
        .ok_or_else(|| format!("package {pkg_name} not in metadata"))?;
    let mut stack = vec![pkg_id];
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(name) = id_to_name.get(id) else {
            continue;
        };
        if *name == "lokai-index"
            || *name == "lokai-lsp"
            || *name == "tetonic-index"
            || *name == "tetonic-lsp"
        {
            hits.push((*name).to_string());
        }
        let Some(node) = resolve
            .iter()
            .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(id))
        else {
            continue;
        };
        if let Some(deps) = node.get("deps").and_then(|d| d.as_array()) {
            for dep in deps {
                if let Some(dep_id) = dep.get("pkg").and_then(|v| v.as_str()) {
                    stack.push(dep_id);
                }
            }
        }
    }
    hits.sort();
    hits.dedup();
    Ok(hits)
}

#[test]
fn lokai_core_has_no_index_or_lsp_in_resolve_graph() {
    let hits = package_depends_on_index_or_lsp("tetonic-core").expect("cargo metadata");
    assert!(
        hits.is_empty(),
        "tetonic-core resolve graph must not include lokai-index/lokai-lsp, found {hits:?}"
    );
}

#[test]
fn lokai_runtime_has_no_index_or_lsp_in_resolve_graph() {
    let hits = package_depends_on_index_or_lsp("tetonic-runtime").expect("cargo metadata");
    assert!(
        hits.is_empty(),
        "tetonic-runtime resolve graph must not include lokai-index/lokai-lsp, found {hits:?}"
    );
}
