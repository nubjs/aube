//! Embedder no-churn lockfile write guard, exercised end-to-end.
//!
//! Lives in its own integration-test binary (= its own process) because
//! the active embedder identity is once-per-process: a unit test inside
//! the crate would race the default (`aube`) identity, whose default for
//! `no_churn_lockfile_write` is `false` (upstream — always write).
//!
//! Contract under test: with an embedder that opts the guard ON, a
//! second write of a graph whose resolved identity equals the on-disk
//! lockfile's leaves the file untouched (same mtime), while a write of a
//! genuinely-changed graph rewrites it.

use std::collections::BTreeMap;

use aube_lockfile::{DepType, DirectDep, LockedPackage, LockfileGraph, LockfileKind, write_lockfile_as};
use aube_manifest::PackageJson;
use aube_util::Embedder;

// Same as AUBE except the no-churn guard is ON. (Distinct lockfile
// basename keeps the debug-assert in `set_embedder` happy and avoids any
// chance of aliasing a foreign name.)
static NO_CHURN_TOOL: Embedder = Embedder {
    name: "nochurn",
    display_name: "nochurn",
    vendor: None,
    version: "1.0.0",
    user_agent: "nochurn/1.0.0",
    self_names: &["nochurn"],
    compatible_names: &["pnpm"],
    lockfile_basename: "nochurn-lock.yaml",
    workspace_yaml: Some("nochurn-workspace.yaml"),
    manifest_namespace: "nochurn",
    env_prefix: Some("NOCHURN"),
    cache_namespace: "nochurn",
    data_namespace: "nochurn",
    canonical_lockfile_always_wins: true,
    runtime_switching: true,
    self_engines_check: true,
    self_update_enabled: true,
    warm_store_verify: true,
    no_churn_lockfile_write: true,
};

fn pkg(name: &str, version: &str, integrity: &str) -> LockedPackage {
    LockedPackage {
        name: name.to_string(),
        version: version.to_string(),
        integrity: Some(integrity.to_string()),
        dep_path: format!("{name}@{version}"),
        ..Default::default()
    }
}

fn graph_with(packages: Vec<LockedPackage>) -> LockfileGraph {
    let mut pkg_map = BTreeMap::new();
    for p in &packages {
        pkg_map.insert(p.dep_path.clone(), p.clone());
    }
    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        packages
            .iter()
            .map(|p| DirectDep {
                name: p.name.clone(),
                dep_path: p.dep_path.clone(),
                dep_type: DepType::Production,
                specifier: Some(format!("^{}", p.version)),
            })
            .collect(),
    );
    LockfileGraph {
        importers,
        packages: pkg_map,
        ..Default::default()
    }
}

fn mtime(path: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn guard_skips_rewrite_when_graph_unchanged_and_writes_when_changed() {
    aube_util::set_embedder(&NO_CHURN_TOOL);
    assert!(
        aube_util::embedder().no_churn_lockfile_write,
        "this test binary must run under the no-churn embedder"
    );

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();
    let manifest = PackageJson::default();

    let graph = graph_with(vec![pkg("foo", "1.0.0", "sha512-AAA==")]);

    // First write creates the lockfile.
    let path = write_lockfile_as(dir.path(), &graph, &manifest, LockfileKind::Pnpm).unwrap();
    assert!(path.exists(), "first write must create the lockfile");
    let first = mtime(&path);

    // mtime resolution on some filesystems is coarse; sleep so a real
    // rewrite would be observable as a distinct timestamp.
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Second write of the SAME graph must be skipped — the resolved
    // graph identity equals what's on disk, so the file is untouched.
    let path2 = write_lockfile_as(dir.path(), &graph, &manifest, LockfileKind::Pnpm).unwrap();
    assert_eq!(path2, path, "skipped write still reports the target path");
    assert_eq!(
        mtime(&path),
        first,
        "no-churn guard must not rewrite a graph-equal lockfile"
    );

    std::thread::sleep(std::time::Duration::from_millis(20));

    // A genuinely changed graph (new package + new integrity) must be
    // written — the guard only suppresses no-ops.
    let changed = graph_with(vec![
        pkg("foo", "1.0.0", "sha512-AAA=="),
        pkg("bar", "2.0.0", "sha512-BBB=="),
    ]);
    write_lockfile_as(dir.path(), &changed, &manifest, LockfileKind::Pnpm).unwrap();
    assert!(
        mtime(&path) > first,
        "a changed graph must rewrite the lockfile"
    );
}
