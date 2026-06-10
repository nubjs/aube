//! Integration test for the embedder settings overlay.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_embedder_overlay` is once-per-process: registering
//! `nodeLinker` here would leak into the `values.rs` unit tests that
//! resolve the same setting from other sources.

use aube_settings::{ResolveCtx, resolved, set_embedder_overlay};
use std::collections::BTreeMap;

fn ctx<'a>(
    ws: &'a BTreeMap<String, yaml_serde::Value>,
    env: &'a [(String, String)],
    cli: &'a [(String, String)],
) -> ResolveCtx<'a> {
    ResolveCtx {
        project_aube_config: &[],
        project_npmrc: &[],
        user_aube_config: &[],
        user_npmrc: &[],
        workspace_yaml: ws,
        env,
        cli,
    }
}

#[test]
fn overlay_ranks_below_cli_and_above_env() {
    set_embedder_overlay(vec![("nodeLinker".to_string(), "hoisted".to_string())]);
    let ws = BTreeMap::new();

    // Overlay alone resolves through the generated accessor.
    let plain = ctx(&ws, &[], &[]);
    assert_eq!(
        resolved::node_linker(&plain),
        resolved::NodeLinker::Hoisted,
        "overlay value must reach the resolved accessor"
    );

    // Overlay outranks the environment.
    let env = vec![("npm_config_node_linker".to_string(), "isolated".to_string())];
    let with_env = ctx(&ws, &env, &[]);
    assert_eq!(
        resolved::node_linker(&with_env),
        resolved::NodeLinker::Hoisted,
        "overlay must win over env"
    );

    // CLI still outranks the overlay.
    let cli = vec![("node-linker".to_string(), "isolated".to_string())];
    let with_cli = ctx(&ws, &env, &cli);
    assert_eq!(
        resolved::node_linker(&with_cli),
        resolved::NodeLinker::Isolated,
        "CLI must win over overlay"
    );
}
