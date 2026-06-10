//! Integration test for the lifecycle-specific user-agent override.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because both `set_user_agent_product` and
//! `set_lifecycle_user_agent_product` are once-per-process; the
//! registrations here must not leak into the binaries asserting the
//! default or product-only identities.

#[test]
fn lifecycle_token_overrides_scripts_only_and_product_identity_survives() {
    aube_util::ua::set_user_agent_product("mytool/2.1.0");
    aube_util::ua::set_lifecycle_user_agent_product("pnpm/10.0.0 mytool/2.1.0 node/v22.0.0");

    let ua = aube_scripts::aube_user_agent();
    assert!(
        ua.starts_with("pnpm/10.0.0 mytool/2.1.0 node/v22.0.0 "),
        "lifecycle token must lead the script UA, got: {ua}"
    );
    assert_eq!(
        ua.split_whitespace().count(),
        5,
        "platform/arch tail must follow the lifecycle token, got: {ua}"
    );

    // The product identity is untouched: stream-time messages and the
    // registry header derive from the product token, not the lifecycle
    // one.
    assert_eq!(
        aube_util::ua::product_name(),
        "mytool",
        "product_name must ignore the lifecycle override"
    );
    assert_eq!(
        aube_util::ua::user_agent_product(),
        Some("mytool/2.1.0"),
        "registry-side product token must ignore the lifecycle override"
    );
}
