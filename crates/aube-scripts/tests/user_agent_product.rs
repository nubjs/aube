//! Integration test for the embedder user-agent product override.
//!
//! Lives in its own integration-test binary — i.e. its own process —
//! because `set_user_agent_product` is once-per-process: registering a
//! product here would leak into the `user_agent_tests` unit tests that
//! assert the default `aube/<version>` token.

#[test]
fn registered_product_replaces_the_default_token_and_keeps_the_platform_tail() {
    aube_util::ua::set_user_agent_product("mytool/2.1.0");
    let ua = aube_scripts::aube_user_agent();
    assert!(
        ua.starts_with("mytool/2.1.0 "),
        "product token must lead the UA, got: {ua}"
    );
    assert!(
        !ua.contains("aube/"),
        "default product must be fully replaced, got: {ua}"
    );
    assert_eq!(
        ua.split_whitespace().count(),
        3,
        "platform/arch tail must survive the override, got: {ua}"
    );
}
