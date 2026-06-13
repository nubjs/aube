//! An embedder whose `manifest_namespace` is `""` ("manifest root") writes
//! map settings (`allowBuilds`, `patchedDependencies`, …) as **top-level**
//! `package.json` keys — never nested under a namespace object, and never
//! under a foreign brand's key (`pnpm`).
//!
//! Lives in its own integration-test binary (= its own process) because the
//! active identity is once-per-process: the in-crate unit tests run under the
//! default `aube` identity (`manifest_namespace="aube"`) and can't flip it.
//!
//! This mirrors nub's profile (`manifest_namespace=""`, `compatible_names=
//! ["pnpm"]`), whose own migration writer emits these settings at the manifest
//! root and whose read side gates the `pnpm` namespace off — so a nested
//! `pnpm.*` write would be orphaned.

use aube_manifest::workspace::edit_setting_map;
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
    cache_namespace: "roottool",
    data_namespace: "roottool",
    canonical_lockfile_always_wins: true,
    runtime_switching: true,
    self_engines_check: true,
    self_update_enabled: true,
    warm_store_verify: true,
    no_churn_lockfile_write: false,
    read_branded_settings_env: true,
};

fn read_manifest(dir: &std::path::Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(dir.join("package.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// A map setting written under a `manifest_namespace=""` embedder lands at the
/// manifest root, an existing root-level entry round-trips (merge), and neither
/// a `""` key nor a foreign `pnpm` namespace is created — even when `pnpm` is
/// already declared in the manifest.
#[test]
fn root_embedder_writes_map_settings_at_manifest_root() {
    aube_util::set_embedder(&ROOT_TOOL);

    let tmp = tempfile::tempdir().unwrap();
    // Pre-existing `pnpm` object present (the case that must NOT divert the
    // write into `pnpm`), plus a prior root-level `allowBuilds` entry that
    // must survive the merge.
    std::fs::write(
        tmp.path().join("package.json"),
        "{\n  \"name\": \"x\",\n  \"pnpm\": {},\n  \"allowBuilds\": { \"old\": true }\n}\n",
    )
    .unwrap();

    edit_setting_map(tmp.path(), "allowBuilds", |m| {
        m.insert("esbuild".to_string(), serde_json::Value::Bool(true));
    })
    .unwrap();

    let value = read_manifest(tmp.path());
    let obj = value.as_object().unwrap();

    // Lands at the manifest ROOT as a top-level map key.
    assert_eq!(
        obj["allowBuilds"]["esbuild"],
        serde_json::Value::Bool(true),
        "new entry must land at top-level allowBuilds, got: {obj:#?}"
    );
    // The pre-existing root-level entry round-trips via the merge.
    assert_eq!(
        obj["allowBuilds"]["old"],
        serde_json::Value::Bool(true),
        "existing root-level entry must survive the write"
    );
    // Never an empty-string namespace key.
    assert!(
        !obj.contains_key(""),
        "must never write an empty-string namespace key, got: {obj:#?}"
    );
    // Never nested under the foreign `pnpm` brand.
    assert!(
        obj.get("pnpm").and_then(|p| p.get("allowBuilds")).is_none(),
        "must never nest the setting under the pnpm namespace, got: {obj:#?}"
    );
}
