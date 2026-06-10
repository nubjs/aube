//! Integration test for the embedder workspace-yaml filename
//! override.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_workspace_yaml_names` is once-per-process: restricting
//! the name list here would leak into the unit tests that exercise the
//! default `aube-workspace.yaml` / `pnpm-workspace.yaml` pair.

use aube_manifest::workspace::{
    WorkspaceConfig, set_workspace_yaml_names, workspace_yaml_existing, workspace_yaml_target,
};

#[test]
fn restricted_name_list_hides_excluded_yamls_and_redirects_fresh_writes() {
    set_workspace_yaml_names(&["pnpm-workspace.yaml"]);
    let dir = tempfile::tempdir().expect("tempdir");

    // An aube-named yaml on disk is invisible once excluded from the
    // list — discovery, typed load, and write-target resolution all
    // skip it.
    std::fs::write(
        dir.path().join("aube-workspace.yaml"),
        "packages:\n  - \"excluded/*\"\n",
    )
    .expect("write aube yaml");
    assert_eq!(
        workspace_yaml_existing(dir.path()),
        None,
        "excluded filename must not count as an existing workspace yaml"
    );
    let config = WorkspaceConfig::load(dir.path()).expect("load");
    assert!(
        config.packages.is_empty(),
        "excluded yaml must not feed config, got packages: {:?}",
        config.packages
    );
    assert_eq!(
        workspace_yaml_target(dir.path()),
        Some(dir.path().join("pnpm-workspace.yaml")),
        "fresh writes must land on the first configured filename"
    );

    // The remaining configured name still round-trips normally.
    let dir2 = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir2.path().join("pnpm-workspace.yaml"),
        "packages:\n  - \"pkgs/*\"\n",
    )
    .expect("write pnpm yaml");
    let config = WorkspaceConfig::load(dir2.path()).expect("load");
    assert_eq!(config.packages, vec!["pkgs/*".to_string()]);
    assert_eq!(
        workspace_yaml_target(dir2.path()),
        Some(dir2.path().join("pnpm-workspace.yaml"))
    );
}
