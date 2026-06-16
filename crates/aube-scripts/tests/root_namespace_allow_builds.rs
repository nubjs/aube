//! Manifest-root embedders such as nub write `allowBuilds` at package.json
//! root after migration. The lifecycle build policy must consume that root map
//! only when the embedder marks its root-native config surface active.

use aube_manifest::PackageJson;
use aube_scripts::{AllowDecision, BuildPolicy};
use aube_util::Embedder;

static ROOT_TOOL: Embedder = Embedder {
    name: "roottool",
    display_name: "roottool",
    vendor: None,
    version: "1.0.0",
    user_agent: "roottool/1.0.0",
    self_names: &["roottool"],
    compatible_names: &["pnpm"],
    lockfile_basename: "roottool-lock.yaml",
    workspace_yaml: None,
    manifest_namespace: "",
    env_prefix: None,
    config_env_prefix: None,
    cache_namespace: "roottool",
    data_namespace: "roottool",
    canonical_lockfile_always_wins: true,
    runtime_switching: true,
    self_engines_check: true,
    self_update_enabled: true,
    warm_store_verify: true,
    no_churn_lockfile_write: false,
    read_branded_settings_env: true,
    primer_ttl: None,
};

fn build_decision(manifest: &PackageJson, name: &str, version: &str) -> AllowDecision {
    let (policy, warnings) =
        BuildPolicy::from_config(&manifest.pnpm_allow_builds(), &[], &[], false);
    assert!(
        warnings.is_empty(),
        "unexpected build-policy warnings: {warnings:?}"
    );
    policy.decide(name, version)
}

#[test]
fn root_allow_builds_feeds_build_policy_only_when_root_surface_is_active() {
    aube_util::set_embedder(&ROOT_TOOL);
    let manifest = PackageJson::parse(
        std::path::Path::new("package.json"),
        r#"{
            "name": "x",
            "allowBuilds": {
                "esbuild": true,
                "sharp": false
            },
            "pnpm": {
                "allowBuilds": {
                    "left-pad": true
                }
            }
        }"#
        .to_string(),
    )
    .unwrap();

    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = false;
        ctx.read_manifest_root_config = false;
    });
    assert_eq!(
        build_decision(&manifest, "esbuild", "0.19.0"),
        AllowDecision::Unspecified
    );

    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = true;
        ctx.read_manifest_root_config = false;
    });
    assert_eq!(
        build_decision(&manifest, "left-pad", "1.3.0"),
        AllowDecision::Allow
    );
    assert_eq!(
        build_decision(&manifest, "esbuild", "0.19.0"),
        AllowDecision::Unspecified
    );

    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = false;
        ctx.read_manifest_root_config = true;
    });
    assert_eq!(
        build_decision(&manifest, "esbuild", "0.19.0"),
        AllowDecision::Allow
    );
    assert_eq!(
        build_decision(&manifest, "sharp", "0.33.0"),
        AllowDecision::Deny
    );
    assert_eq!(
        build_decision(&manifest, "left-pad", "1.3.0"),
        AllowDecision::Unspecified
    );
}
