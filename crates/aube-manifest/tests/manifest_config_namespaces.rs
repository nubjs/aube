//! Integration test for the embedder `package.json` config-namespace
//! override.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_manifest_config_namespaces` is once-per-process:
//! restricting the list here would leak into the unit tests that
//! exercise the default `pnpm` + `aube` pair.

use aube_manifest::{PackageJson, set_manifest_config_namespaces};
use std::path::Path;

#[test]
fn restricted_namespace_list_ignores_excluded_objects_but_keeps_top_level_keys() {
    set_manifest_config_namespaces(&["pnpm"]);

    let manifest = PackageJson::parse(
        Path::new("package.json"),
        serde_json::json!({
            "name": "demo",
            "version": "1.0.0",
            "trustedDependencies": ["esbuild"],
            "pnpm": {
                "onlyBuiltDependencies": ["from-pnpm"],
                "patchedDependencies": { "left-pad@1.3.0": "patches/a.patch" }
            },
            "aube": {
                "onlyBuiltDependencies": ["from-aube"],
                "patchedDependencies": { "left-pad@1.3.0": "patches/b.patch" }
            }
        })
        .to_string(),
    )
    .expect("parse");

    assert_eq!(
        manifest.pnpm_only_built_dependencies(),
        vec!["from-pnpm".to_string()],
        "excluded `aube` object must not feed namespaced config"
    );
    assert_eq!(
        manifest
            .pnpm_patched_dependencies()
            .get("left-pad@1.3.0")
            .map(String::as_str),
        Some("patches/a.patch"),
        "excluded `aube` object must not win map-shaped merges"
    );
    assert_eq!(
        manifest.trusted_dependencies(),
        vec!["esbuild".to_string()],
        "top-level compatibility keys are independent of the namespace list"
    );
}
