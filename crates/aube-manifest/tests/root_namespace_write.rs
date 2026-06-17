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

use aube_manifest::{
    AllowBuildRaw, PackageJson, workspace::edit_setting_map, workspace::set_allow_builds,
};
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

#[test]
fn root_embedder_reads_root_allow_builds_only_when_root_surface_is_active() {
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

    // A non-pnpm incumbent under a manifest-root embedder gates both pnpm and
    // root-native config off. This preserves compat projects where root
    // `allowBuilds` is not the active package manager's surface.
    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = false;
        ctx.read_manifest_root_config = false;
    });
    assert!(manifest.pnpm_allow_builds().is_empty());

    // Pnpm/fresh mode reads only pnpm-branded config, not the manifest-root
    // setting that belongs to the root embedder identity.
    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = true;
        ctx.read_manifest_root_config = false;
    });
    let pnpm = manifest.pnpm_allow_builds();
    assert!(matches!(
        pnpm.get("left-pad"),
        Some(AllowBuildRaw::Bool(true))
    ));
    assert!(!pnpm.contains_key("esbuild"));

    // NubIdentity-style mode gates pnpm off and reads root `allowBuilds` as the
    // native config surface produced by `pm use nub`.
    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = false;
        ctx.read_manifest_root_config = true;
    });
    let root = manifest.pnpm_allow_builds();
    assert!(matches!(
        root.get("esbuild"),
        Some(AllowBuildRaw::Bool(true))
    ));
    assert!(matches!(
        root.get("sharp"),
        Some(AllowBuildRaw::Bool(false))
    ));
    assert!(!root.contains_key("left-pad"));
}

/// The approve-builds heal gap: under a manifest-root embedder on the
/// pnpm-compat/fresh surface (`read_branded_pnpm_config` on,
/// `read_manifest_root_config` off — the common case), `set_allow_builds`
/// must NOT write the top-level `package.json#allowBuilds` key the read side
/// ignores there. It writes the (pnpm) workspace yaml the reader honors, so a
/// subsequent install actually sees the approval. Round-trips through the read
/// side to prove the write is visible.
#[test]
fn set_allow_builds_writes_yaml_on_pnpm_surface_not_unread_root_key() {
    aube_util::set_embedder(&ROOT_TOOL);
    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = true;
        ctx.read_manifest_root_config = false;
    });

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("package.json"),
        "{\n  \"name\": \"x\",\n  \"dependencies\": { \"core-js\": \"3.37.1\" }\n}\n",
    )
    .unwrap();

    let written = set_allow_builds(tmp.path(), &["core-js".to_string()], true).unwrap();

    // No workspace yaml existed, so the write must create pnpm-workspace.yaml
    // (where the read side looks on this surface), NOT a top-level package.json
    // key the reader gates off.
    assert_eq!(
        written.file_name().and_then(|n| n.to_str()),
        Some("pnpm-workspace.yaml"),
        "expected the approval to land in pnpm-workspace.yaml on the pnpm-compat surface, got: {written:?}"
    );
    let manifest = read_manifest(tmp.path());
    assert!(
        manifest.get("allowBuilds").is_none(),
        "must not write an unread top-level allowBuilds key, got: {manifest:#?}"
    );
    let yaml = std::fs::read_to_string(tmp.path().join("pnpm-workspace.yaml")).unwrap();
    assert!(
        yaml.contains("allowBuilds:") && yaml.contains("core-js"),
        "pnpm-workspace.yaml must record the approval, got:\n{yaml}"
    );
}

/// Under nub identity (`read_manifest_root_config` on, pnpm surface off), the
/// reader DOES read the top-level key, so `set_allow_builds` writes it there —
/// no spurious pnpm-workspace.yaml emitted (which would be a brand leak on the
/// nub-identity surface).
#[test]
fn set_allow_builds_writes_root_key_under_nub_identity() {
    aube_util::set_embedder(&ROOT_TOOL);
    aube_util::update_engine_context(|ctx| {
        ctx.read_branded_pnpm_config = false;
        ctx.read_manifest_root_config = true;
    });

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), "{\n  \"name\": \"x\"\n}\n").unwrap();

    let written = set_allow_builds(tmp.path(), &["core-js".to_string()], true).unwrap();

    assert_eq!(
        written.file_name().and_then(|n| n.to_str()),
        Some("package.json"),
        "nub identity reads the top-level key, so it must write package.json, got: {written:?}"
    );
    assert!(
        !tmp.path().join("pnpm-workspace.yaml").exists(),
        "must not emit a pnpm-workspace.yaml under nub identity"
    );
    let manifest = read_manifest(tmp.path());
    assert_eq!(
        manifest["allowBuilds"]["core-js"],
        serde_json::Value::Bool(true),
        "approval must land in the top-level allowBuilds the nub-identity reader consults"
    );
}
