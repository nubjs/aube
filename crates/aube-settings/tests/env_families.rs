//! Integration test for the env-family restriction.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_env_families` is once-per-process: narrowing the
//! families here would leak into every other test that resolves
//! settings from the environment.

use aube_settings::{ResolveCtx, resolved};
use aube_util::env::{EnvFamilies, set_env_families};
use std::collections::BTreeMap;

fn ctx<'a>(
    ws: &'a BTreeMap<String, yaml_serde::Value>,
    env: &'a [(String, String)],
) -> ResolveCtx<'a> {
    ResolveCtx {
        project_aube_config: &[],
        project_npmrc: &[],
        user_aube_config: &[],
        user_npmrc: &[],
        workspace_yaml: ws,
        env,
        cli: &[],
    }
}

#[test]
fn npm_only_restriction_masks_aube_aliases_but_not_npm_config() {
    set_env_families(EnvFamilies::NPM);
    let ws = BTreeMap::new();

    // AUBE_* alias is invisible: the accessor falls through to the
    // generated default instead of honoring the env value.
    let aube_env = vec![("AUBE_NODE_LINKER".to_string(), "hoisted".to_string())];
    assert_eq!(
        resolved::node_linker(&ctx(&ws, &aube_env)),
        resolved::NodeLinker::Isolated,
        "AUBE_NODE_LINKER must be ignored when only the npm family is enabled"
    );

    // npm_config_* alias keeps working.
    let npm_env = vec![("npm_config_node_linker".to_string(), "hoisted".to_string())];
    assert_eq!(
        resolved::node_linker(&ctx(&ws, &npm_env)),
        resolved::NodeLinker::Hoisted,
        "npm_config_node_linker must keep resolving under the npm-only restriction"
    );
}
