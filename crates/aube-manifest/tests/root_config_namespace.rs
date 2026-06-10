//! Integration test for the `""` (manifest-root) config namespace.
//!
//! Own integration-test binary — own process — because
//! `set_manifest_config_namespaces` is once-per-process; configuring
//! the root-only list here must not leak into the unit tests that
//! exercise the default `pnpm` + `aube` pair.

use aube_manifest::{PackageJson, set_manifest_config_namespaces};
use std::path::Path;

#[test]
fn root_namespace_reads_top_level_config_and_drops_pnpm_objects() {
    set_manifest_config_namespaces(&[""]);

    let manifest = PackageJson::parse(
        Path::new("package.json"),
        serde_json::json!({
            "name": "demo",
            "version": "1.0.0",
            // Top-level three-state allowBuilds map (bun-style home,
            // pnpm-style model): true = run, false = acknowledged
            // skip, unlisted = skip + warn.
            "allowBuilds": { "esbuild": true, "sketchy-pkg": false },
            "patchedDependencies": { "left-pad@1.3.0": "patches/a.patch" },
            "pnpm": {
                "allowBuilds": { "from-pnpm": true },
                "patchedDependencies": { "left-pad@1.3.0": "patches/pnpm.patch" }
            }
        })
        .to_string(),
    )
    .expect("parse");

    let allow_builds = manifest.pnpm_allow_builds();
    assert_eq!(
        allow_builds.get("esbuild"),
        Some(&aube_manifest::AllowBuildRaw::Bool(true)),
        "top-level allowBuilds true entry must be read via the root namespace"
    );
    assert_eq!(
        allow_builds.get("sketchy-pkg"),
        Some(&aube_manifest::AllowBuildRaw::Bool(false)),
        "top-level allowBuilds false entry must be read via the root namespace"
    );
    assert!(
        !allow_builds.contains_key("from-pnpm"),
        "with the root-only list, the pnpm object must be unread"
    );

    // patchedDependencies: the dedicated bun-compat top-level read and
    // the root namespace agree; the excluded pnpm object must not win.
    assert_eq!(
        manifest
            .bun_patched_dependencies()
            .get("left-pad@1.3.0")
            .map(String::as_str),
        Some("patches/a.patch"),
    );
    assert_eq!(
        manifest
            .pnpm_patched_dependencies()
            .get("left-pad@1.3.0")
            .map(String::as_str),
        Some("patches/a.patch"),
        "root namespace must serve patchedDependencies from the top level only"
    );
}
