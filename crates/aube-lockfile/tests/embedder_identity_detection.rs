//! Integration test for the embedder-parameterized detection carve-outs:
//! custom self-names + strict canonical-lockfile coexistence.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because the three registrations (`set_aube_lock_base_filename`,
//! `set_detection_self_names`, `set_canonical_lockfile_always_wins`) are
//! once-per-process and would poison the unit tests asserting the
//! defaults.

use aube_lockfile::{Error, LockfileKind, ResolvedLockfileKind, resolve_project_lockfile_kind};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

#[test]
fn strict_embedder_identity_decision_rows() {
    aube_lockfile::set_aube_lock_base_filename("lock.yaml");
    aube_lockfile::set_detection_self_names(&["mytool"]);
    aube_lockfile::set_canonical_lockfile_always_wins(false);

    // lock.yaml + no declaration → the canonical kind (embedder identity).
    let d = project(&[
        ("package.json", r#"{"name":"t"}"#),
        ("lock.yaml", "lockfileVersion: '9.0'\n"),
    ]);
    assert_eq!(
        resolve_project_lockfile_kind(d.path()).unwrap(),
        ResolvedLockfileKind::Existing(LockfileKind::Aube)
    );

    // lock.yaml beside a foreign lockfile, no declaration → loud ambiguity
    // (the upstream always-wins carve-out is demoted under strict identity).
    let d = project(&[
        ("package.json", r#"{"name":"t"}"#),
        ("lock.yaml", "lockfileVersion: '9.0'\n"),
        ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n"),
    ]);
    let err = resolve_project_lockfile_kind(d.path()).unwrap_err();
    let Error::AmbiguousLockfiles { found } = &err else {
        panic!("expected AmbiguousLockfiles, got {err:?}");
    };
    assert!(
        found.contains("lock.yaml") && found.contains("pnpm-lock.yaml"),
        "ambiguity must name both files: {found}"
    );

    // Declared pnpm + only lock.yaml → contradiction naming the file.
    let d = project(&[
        (
            "package.json",
            r#"{"name":"t","packageManager":"pnpm@10.0.0"}"#,
        ),
        ("lock.yaml", "lockfileVersion: '9.0'\n"),
    ]);
    let err = resolve_project_lockfile_kind(d.path()).unwrap_err();
    let Error::DeclarationMismatch { found, .. } = &err else {
        panic!("expected DeclarationMismatch, got {err:?}");
    };
    assert_eq!(found, "lock.yaml");

    // The registered self-name behaves exactly like a declared `aube`
    // upstream: accepts an existing foreign format, pins the canonical
    // format when fresh.
    let d = project(&[
        (
            "package.json",
            r#"{"name":"t","packageManager":"mytool@1.0.0"}"#,
        ),
        ("package-lock.json", "{}"),
    ]);
    assert_eq!(
        resolve_project_lockfile_kind(d.path()).unwrap(),
        ResolvedLockfileKind::Existing(LockfileKind::Npm)
    );
    let d = project(&[(
        "package.json",
        r#"{"name":"t","packageManager":"mytool@1.0.0"}"#,
    )]);
    assert_eq!(
        resolve_project_lockfile_kind(d.path()).unwrap(),
        ResolvedLockfileKind::DeclaredFresh(LockfileKind::Aube)
    );
}
