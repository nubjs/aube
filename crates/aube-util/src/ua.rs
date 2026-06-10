//! Process-wide user-agent product identity.
//!
//! Aube identifies itself in two places: the `User-Agent` header on
//! registry requests and the `npm_config_user_agent` variable exported
//! to lifecycle scripts. Both default to `aube/<version>`. A tool that
//! embeds aube's command layer as a library is the running product
//! from the perspective of both audiences — dep postinstalls sniff the
//! first product token of `npm_config_user_agent` to detect the
//! package manager driving them, and registries key cache and abuse
//! heuristics off the UA — so it can register its own product token
//! here once per process.

use std::sync::OnceLock;

static PRODUCT: OnceLock<String> = OnceLock::new();

/// Override the leading product token(s) of aube's user-agent strings,
/// e.g. `"mytool/2.1.0"`. The platform tail each consumer appends is
/// preserved; only the product identity changes. Multiple
/// space-separated `name/version` tokens are fine when the embedder
/// wants to keep aube visible (`"mytool/2.1.0 aube/1.18.2"`).
///
/// Idempotent — second calls are silently ignored, matching the other
/// process-global `set_*` helpers: consumers cache the assembled UA in
/// a `OnceLock`, so flipping the product mid-process would produce
/// split-brain strings.
pub fn set_user_agent_product(product: impl Into<String>) {
    let _ = PRODUCT.set(product.into());
}

/// The registered product token(s), if an embedder set one. Consumers
/// fall back to their own `aube/<version>` when this is `None`.
pub fn user_agent_product() -> Option<&'static str> {
    PRODUCT.get().map(String::as_str)
}
