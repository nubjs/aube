//! Integration test for the embedder `engines.aube` validation
//! toggle.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_aube_engine_check` is once-per-process: disabling the
//! check here would leak into the unit tests that assert the default
//! `engines.aube` mismatch behavior.

use aube::engines::{Engine, check_root, set_aube_engine_check};
use std::path::Path;

#[test]
fn disabled_aube_engine_check_skips_aube_but_keeps_node() {
    set_aube_engine_check(false);

    let manifest = aube_manifest::PackageJson::parse(
        Path::new("package.json"),
        serde_json::json!({
            "name": "demo",
            "version": "1.0.0",
            "engines": { "node": ">=99999", "aube": ">=99999" }
        })
        .to_string(),
    )
    .expect("parse");

    let mismatches = check_root(&manifest, Some("18.0.0"));
    assert_eq!(
        mismatches.len(),
        1,
        "only engines.node may flag once the aube check is off, got {mismatches:?}"
    );
    assert_eq!(mismatches[0].engine, Engine::Node);
}
