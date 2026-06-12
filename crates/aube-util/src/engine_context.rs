//! Runtime embedder seam — the per-invocation counterpart to [`Embedder`].
//!
//! [`Embedder`](crate::identity::Embedder) is the *compile-time* embedder
//! profile: branding plus the behavior toggles fixed for the life of the
//! binary. But an embedder (e.g. nub) also computes a handful of values
//! *per project / per invocation* — which override sources apply, whether a
//! pnpm-named file is the active PM's and may be read, the PATH/env overlay
//! that routes lifecycle scripts through a provisioned runtime. A compile-time
//! const cannot carry those. [`EngineContext`] is their home: a process-global
//! struct the embedder populates as a run progresses, which aube's seam
//! read-sites consult.
//!
//! Unlike [`Embedder`] (selected once, at the entry point, before any command
//! runs), the context's fields are computed at *different phases* of a run —
//! some at startup, some after the manifest is parsed, some after settings
//! resolve. So it is backed by a `RwLock` and populated incrementally:
//! [`update_engine_context`] mutates individual fields in place, while
//! [`set_engine_context`] replaces the whole struct. [`engine_context`] returns
//! a snapshot clone.
//!
//! **Default = upstream-neutral for every field.** Standalone aube (and any
//! test) that never touches the context gets exactly upstream behavior:
//! [`EngineContext::default`] reproduces it field-for-field. This is the
//! runtime analogue of [`AUBE`](crate::identity::AUBE) being the unset
//! [`Embedder`] fallback.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

/// Per-invocation values an embedder computes and hands to aube.
///
/// Every field defaults to aube's upstream behavior, so an unset context is
/// behavior-neutral. Populated incrementally across a run's phases via
/// [`update_engine_context`] (field-level) or [`set_engine_context`]
/// (whole-struct replace).
#[derive(Clone, Debug)]
pub struct EngineContext {
    /// Replacement dependency-override source. `Some(map)` makes the supplied
    /// map the *sole* override source — `PackageJson::overrides_map` returns it
    /// verbatim instead of folding the manifest's `resolutions` /
    /// `pnpm.overrides` / top-level `overrides` with the built-in precedence.
    /// `None` (default) leaves upstream behavior untouched (fold every source).
    ///
    /// The embedder seam for tools that scope which override dialects apply per
    /// project (e.g. honoring only the active package manager's native field).
    /// aube assigns no policy — it consumes whatever map the embedder computed,
    /// typically from `PackageJson::tagged_overrides`.
    pub embedder_overrides: Option<BTreeMap<String, String>>,

    /// Whether Bun's top-level `trustedDependencies` array contributes to the
    /// lifecycle build allowlist. `true` (default) preserves upstream behavior
    /// — `trustedDependencies` unions into the allowlist. `false` makes
    /// `PackageJson::trusted_dependencies` return an empty list, for embedders
    /// whose active package manager ignores the field (every PM except Bun, and
    /// Bun itself from the version that dropped it).
    pub trusted_dependencies_honored: bool,

    /// Whether aube reads the *branded pnpm* config-compat surface. `true`
    /// (default) is upstream behavior: aube consults pnpm's surfaces alongside
    /// its own. This single posture drives all three pnpm-branded read-sites
    /// together —
    ///
    /// 1. `pnpm-workspace.yaml` is included in the workspace-yaml candidate
    ///    list (probed/read for workspace settings);
    /// 2. the `pnpm` `package.json` config namespace is consulted (folded with
    ///    the tool's own `aube.*` namespace);
    /// 3. pnpm's global `~/.config/pnpm/auth.ini` is read and its tokens
    ///    merged.
    ///
    /// The actual branded values (`"pnpm-workspace.yaml"`, the `"pnpm"`
    /// namespace) are aube's own compiled-in pnpm-compat knowledge; this bool
    /// only *gates* whether they apply. An embedder whose active PM isn't pnpm
    /// sets `false`: under a non-pnpm incumbent those pnpm-named surfaces are
    /// another tool's state and must not be read (a name-based policy). The
    /// tool's own branded YAML/namespace, `.npmrc`, and `npmrcAuthFile`
    /// sources are unaffected.
    pub read_branded_pnpm_config: bool,

    /// Whether the cwd-default `.pnpmfile` is detected. `true` (default) is
    /// upstream. An embedder under a non-pnpm incumbent sets `false`: a stray
    /// `.pnpmfile` is another tool's resolution-shaping config and is not
    /// honored. Explicit `pnpmfilePath` overrides are unaffected.
    pub pnpmfile_default_enabled: bool,

    /// PATH entries prepended (in order, ahead of the existing PATH) to every
    /// lifecycle spawn. An embedder places a runtime shim dir first so a bare
    /// `node` in a build script resolves to the augmented runtime. Default
    /// empty = no-op. The embedder owns the *source*; aube copies it into
    /// `ScriptSettings` at settings-resolution time and the spawn path composes
    /// it onto PATH.
    pub path_prepends: Vec<PathBuf>,

    /// Environment overlay applied verbatim to every lifecycle spawn (set last,
    /// so it outranks the settings-derived keys). Generic by design — aube
    /// assigns no meaning to the keys; an embedder fills it to route scripts
    /// through a provisioned/augmented runtime (e.g. point `NODE` at a shim,
    /// pin `npm_node_execpath`, inject a preload via `NODE_OPTIONS`). Default
    /// empty = behavior-preserving.
    pub env_overlay: Vec<(OsString, OsString)>,

    /// Replacement lifecycle `npm_config_user_agent` product token. `None`
    /// (default) falls back to the compile-time [`Embedder::user_agent`] —
    /// standalone aube reports `aube/<version>`. An embedder sets `Some` when
    /// the product string is genuinely *runtime*: nub emits a per-mode UA
    /// embedding the project's RESOLVED node version (e.g.
    /// `pnpm/x nub/x node/vX`), which can't be a compile-time literal. Read at
    /// the lifecycle-UA seam in `aube-scripts`.
    ///
    /// [`Embedder::user_agent`]: crate::identity::Embedder::user_agent
    pub lifecycle_user_agent_product: Option<String>,
}

impl Default for EngineContext {
    /// Upstream-neutral defaults — an unset context reproduces standalone aube
    /// behavior for every seam.
    fn default() -> Self {
        Self {
            embedder_overrides: None,
            trusted_dependencies_honored: true,
            read_branded_pnpm_config: true,
            pnpmfile_default_enabled: true,
            path_prepends: Vec::new(),
            env_overlay: Vec::new(),
            lifecycle_user_agent_product: None,
        }
    }
}

static ACTIVE: OnceLock<RwLock<EngineContext>> = OnceLock::new();

fn active() -> &'static RwLock<EngineContext> {
    ACTIVE.get_or_init(|| RwLock::new(EngineContext::default()))
}

/// A snapshot clone of the active engine context, or
/// [`EngineContext::default`] when nothing was set. Never panics.
pub fn engine_context() -> EngineContext {
    match active().read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Replace the whole engine context. Use when an embedder computes every field
/// at once; prefer [`update_engine_context`] when populating fields across the
/// different phases of a run.
pub fn set_engine_context(context: EngineContext) {
    match active().write() {
        Ok(mut guard) => *guard = context,
        Err(poisoned) => *poisoned.into_inner() = context,
    }
}

/// Mutate the active engine context in place. The closure receives a mutable
/// reference to the current context (its prior fields preserved), so an
/// embedder can populate one field per phase without clobbering the others.
pub fn update_engine_context(f: impl FnOnce(&mut EngineContext)) {
    match active().write() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With nothing set, every field is upstream-neutral. This is the
    /// behavior-neutrality contract: an embedder that sets nothing gets aube.
    /// (Mirrors `identity::tests::embedder_unset_is_aube`.)
    #[test]
    fn default_is_upstream_neutral() {
        let ctx = EngineContext::default();
        assert_eq!(ctx.embedder_overrides, None);
        assert!(ctx.trusted_dependencies_honored);
        assert!(ctx.read_branded_pnpm_config);
        assert!(ctx.pnpmfile_default_enabled);
        assert!(ctx.path_prepends.is_empty());
        assert!(ctx.env_overlay.is_empty());
        assert_eq!(ctx.lifecycle_user_agent_product, None);
    }
}
