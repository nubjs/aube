//! Integration test for the disabled workspace-yaml surface (the
//! empty list passed to `set_workspace_yaml_names`).
//!
//! Own integration-test binary — own process — because the override
//! is once-per-process and the unit tests rely on the default
//! `aube-workspace.yaml` / `pnpm-workspace.yaml` pair.

use aube_manifest::workspace::{
    self, ConfigWriteTarget, WorkspaceConfig, config_write_target, set_workspace_yaml_names,
    workspace_yaml_existing, workspace_yaml_target,
};

#[test]
fn empty_name_list_disables_reads_and_routes_writes_to_package_json() {
    set_workspace_yaml_names(&[]);

    let dir = tempfile::tempdir().unwrap();
    // A stray pnpm-workspace.yaml on disk must be deliberately unread.
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "packages:\n  - 'pkgs/*'\nnodeLinker: hoisted\n",
    )
    .unwrap();

    let typed = WorkspaceConfig::load(dir.path()).expect("load");
    assert!(
        typed.packages.is_empty() && typed.node_linker.is_none(),
        "typed load must return defaults with the surface disabled"
    );
    assert!(
        workspace::load_raw(dir.path())
            .expect("load_raw")
            .is_empty(),
        "raw settings map must be empty with the surface disabled"
    );
    let (both_typed, both_raw) = workspace::load_both(dir.path()).expect("load_both");
    assert!(both_typed.packages.is_empty() && both_raw.is_empty());

    assert_eq!(workspace_yaml_existing(dir.path()), None);
    assert_eq!(
        workspace_yaml_target(dir.path()),
        None,
        "no filename exists a fresh yaml could be created under"
    );
    assert_eq!(
        config_write_target(dir.path()),
        ConfigWriteTarget::PackageJson,
        "config writes must route to package.json with the surface disabled"
    );
}
