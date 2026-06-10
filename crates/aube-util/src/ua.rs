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
static LIFECYCLE_PRODUCT: OnceLock<String> = OnceLock::new();

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

/// Override the product token(s) of the `npm_config_user_agent`
/// exported to lifecycle scripts, *independently* of the product
/// identity registered via [`set_user_agent_product`].
///
/// The two audiences a UA serves can legitimately need different
/// answers: dep postinstall sniffers (`which-pm-runs`, `only-allow`,
/// create-* scaffolders) key behavior off the first product token of
/// `npm_config_user_agent` — they want to know which package manager's
/// *role* is being played — while registries and stream-time messages
/// identify the tool actually running. An embedder serving an
/// incumbent PM's project can register that PM's token here (e.g.
/// `"pnpm/10.0.0 mytool/2.1.0"`) while its registry traffic and
/// command hints keep its own identity from [`set_user_agent_product`].
///
/// Only [`aube_user_agent`](https://docs.rs/aube-scripts) (the
/// lifecycle-script export) consults this; the registry `User-Agent`
/// header and [`product_name`] never do. The platform tail is
/// appended by the consumer, exactly as for the product token. When
/// unset, the lifecycle UA falls back to the registered product
/// token, then to `aube/<version>` — upstream behavior is unchanged.
///
/// Idempotent — second calls are silently ignored, same as
/// [`set_user_agent_product`].
pub fn set_lifecycle_user_agent_product(product: impl Into<String>) {
    let _ = LIFECYCLE_PRODUCT.set(product.into());
}

/// The registered lifecycle-script product token(s), if an embedder
/// set one. Consumers fall back to [`user_agent_product`].
pub fn lifecycle_user_agent_product() -> Option<&'static str> {
    LIFECYCLE_PRODUCT.get().map(String::as_str)
}

/// The product *name* stream-time messages should print when they name
/// the running tool — the name half of the first registered
/// `name/version` token, or `"aube"` when no embedder registered one.
///
/// Command hints (``Run `aube deprecations` ``), lock-wait notices, and
/// similar prose name the tool the user actually invoked; an embedder
/// that registered its identity for the UA and the install header is
/// that tool, so these messages follow the same registration (the
/// audience contract from [`set_user_agent_product`]).
pub fn product_name() -> &'static str {
    user_agent_product()
        .and_then(|p| p.split_whitespace().next())
        .map(|token| token.split('/').next().unwrap_or(token))
        .unwrap_or("aube")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test fn so the unset→set ordering on the process-global
    /// `PRODUCT` is deterministic (parallel tests would race it).
    #[test]
    fn product_name_defaults_to_aube_and_follows_the_registered_token() {
        assert_eq!(product_name(), "aube", "unset must fall back to aube");
        set_user_agent_product("mytool/2.1.0 aube/1.18.2");
        assert_eq!(
            product_name(),
            "mytool",
            "name half of the first registered token"
        );
    }
}
