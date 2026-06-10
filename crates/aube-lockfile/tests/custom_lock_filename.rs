//! Embedder-configured lockfile filename, exercised end-to-end.
//!
//! Lives in its own integration-test binary (= its own process)
//! because `set_aube_lock_base_filename` is once-per-process: unit
//! tests inside the crate would race the default-initialized
//! `OnceLock`. Same pattern as aube-manifest's
//! `manifest_config_namespaces` test.

use aube_lockfile::{
    LockfileKind, aube_lock_base_filename, aube_lock_filename, detect_existing_lockfile_kind,
    pnpm_lock_filename, resolve_project_lockfile_kind, set_aube_lock_base_filename,
    write_lockfile_as,
};

#[test]
fn configured_filename_drives_naming_detection_and_writes() {
    // Invalid values are ignored: collisions with foreign lockfile
    // names, path separators, bare extension.
    set_aube_lock_base_filename("pnpm-lock.yaml");
    set_aube_lock_base_filename("sub/lock.yaml");
    set_aube_lock_base_filename(".yaml");
    // First valid value wins; later calls are no-ops.
    set_aube_lock_base_filename("lock.yaml");
    set_aube_lock_base_filename("other-lock.yaml");
    assert_eq!(aube_lock_base_filename(), "lock.yaml");
    assert_eq!(LockfileKind::Aube.filename(), "lock.yaml");

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), r#"{"name":"t"}"#).unwrap();

    // Naming: branch lockfiles are off, so the resolved filename is
    // the configured base; the pnpm mapping stays pnpm's own name.
    assert_eq!(aube_lock_filename(dir.path()), "lock.yaml");
    assert_eq!(pnpm_lock_filename(dir.path()), "pnpm-lock.yaml");

    // Write path: the Aube kind lands at the configured name.
    let graph = aube_lockfile::LockfileGraph::default();
    let manifest = aube_manifest::PackageJson::default();
    let written = write_lockfile_as(dir.path(), &graph, &manifest, LockfileKind::Aube).unwrap();
    assert_eq!(written, dir.path().join("lock.yaml"));
    assert!(written.exists(), "lock.yaml must exist after write");
    assert!(
        !dir.path().join("aube-lock.yaml").exists(),
        "the default filename must not be written once overridden"
    );

    // Detection: the configured name ranks top of the precedence
    // order — above pnpm-lock.yaml — and the own-file carve-out in
    // declaration-aware resolution still applies.
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    assert_eq!(
        detect_existing_lockfile_kind(dir.path()),
        Some(LockfileKind::Aube),
        "configured lock.yaml must outrank pnpm-lock.yaml"
    );
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"t","packageManager":"pnpm@10.0.0"}"#,
    )
    .unwrap();
    assert_eq!(
        resolve_project_lockfile_kind(dir.path()).unwrap(),
        aube_lockfile::ResolvedLockfileKind::Existing(LockfileKind::Aube),
        "the configured own-file must win over a pnpm declaration"
    );
}
