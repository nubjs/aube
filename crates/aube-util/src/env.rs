use std::path::PathBuf;
use std::sync::OnceLock;

/// Families of environment variables aube consults for settings and
/// operational toggles. An embedding caller can restrict which
/// families are read at all — e.g. a tool that drives aube as a
/// library may want the npm-compatible `npm_config_*` surface honored
/// while `AUBE_*` variables stay invisible to the host process's
/// environment. The default is [`EnvFamilies::ALL`], which preserves
/// standalone-CLI behavior exactly.
///
/// The restriction applies to settings-class lookups: the generated
/// accessors in `aube-settings` and the directly-read operational
/// `AUBE_*` variables routed through [`var`] / [`var_os`]. It does
/// *not* apply to infrastructure base-directory variables (`HOME`,
/// `USERPROFILE`, `XDG_*`) — those locate the machine's filesystem
/// layout rather than configure aube, and masking them would break
/// path resolution for every caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvFamilies(u8);

impl EnvFamilies {
    /// No environment input at all.
    pub const NONE: Self = Self(0);
    /// `npm_config_*` / `NPM_CONFIG_*` — the npm-compatible env
    /// surface shared with npm, pnpm, and yarn.
    pub const NPM: Self = Self(1 << 0);
    /// `AUBE_*` — aube's own settings aliases and operational
    /// variables (`AUBE_HOME`, `AUBE_AUTH_TOKEN`, kill switches, …).
    pub const AUBE: Self = Self(1 << 1);
    /// Ecosystem-neutral variables aube consults without owning them
    /// (`CI`, proxy variables, …).
    pub const EXTERNAL: Self = Self(1 << 2);
    /// Every family — the default, matching standalone-CLI behavior.
    pub const ALL: Self = Self((1 << 0) | (1 << 1) | (1 << 2));

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Process-wide env-family restriction. `None` (never set) means
/// [`EnvFamilies::ALL`].
static ENV_FAMILIES: OnceLock<EnvFamilies> = OnceLock::new();

/// Restrict which env-variable families aube reads, once per process,
/// before any settings resolution or command runs.
///
/// Idempotent — second calls are silently ignored, matching the other
/// process-global `set_*` helpers: several call sites cache their env
/// lookup in a `OnceLock`/`LazyLock`, so flipping the restriction
/// mid-process would produce split-brain reads.
pub fn set_env_families(families: EnvFamilies) {
    let _ = ENV_FAMILIES.set(families);
}

/// The active env-family restriction ([`EnvFamilies::ALL`] unless an
/// embedder narrowed it via [`set_env_families`]).
pub fn env_families() -> EnvFamilies {
    ENV_FAMILIES.get().copied().unwrap_or(EnvFamilies::ALL)
}

/// Classify a variable name into its [`EnvFamilies`] bit.
pub fn env_family_of(name: &str) -> EnvFamilies {
    if name.starts_with("AUBE_") {
        EnvFamilies::AUBE
    } else if name.starts_with("npm_config_") || name.starts_with("NPM_CONFIG_") {
        EnvFamilies::NPM
    } else {
        EnvFamilies::EXTERNAL
    }
}

/// True when `name`'s family is enabled under the active restriction.
pub fn env_family_enabled(name: &str) -> bool {
    env_families().contains(env_family_of(name))
}

/// [`std::env::var`] gated by the env-family restriction. Every
/// settings-class or operational variable read (anything a user or CI
/// sets to change aube's behavior) goes through here or [`var_os`];
/// purely diagnostic variables (`AUBE_DIAG_*`, `AUBE_BENCH_*`) read
/// `std::env` directly because they only affect what gets logged,
/// never what aube does.
pub fn var(name: &str) -> Option<String> {
    if !env_family_enabled(name) {
        return None;
    }
    std::env::var(name).ok()
}

/// [`std::env::var_os`] gated by the env-family restriction. See
/// [`var`].
pub fn var_os(name: &str) -> Option<std::ffi::OsString> {
    if !env_family_enabled(name) {
        return None;
    }
    std::env::var_os(name)
}

pub fn is_ci() -> bool {
    var_os("CI").is_some()
}

pub fn home_dir() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("HOME") {
        return Some(h.into());
    }
    #[cfg(windows)]
    if let Some(h) = std::env::var_os("USERPROFILE") {
        return Some(h.into());
    }
    None
}

fn non_empty_path_var(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

pub fn xdg_config_home() -> Option<PathBuf> {
    non_empty_path_var("XDG_CONFIG_HOME")
}

pub fn xdg_data_home() -> Option<PathBuf> {
    non_empty_path_var("XDG_DATA_HOME")
}

pub fn xdg_cache_home() -> Option<PathBuf> {
    non_empty_path_var("XDG_CACHE_HOME")
}

#[cfg(test)]
mod tests {
    use super::*;

    // `set_env_families` itself is exercised in an isolated
    // integration-test process (`aube-settings/tests/env_families.rs`)
    // because the OnceLock is process-global — setting it here would
    // leak into every other unit test in this binary.
    #[test]
    fn env_family_classification_and_bitset_ops() {
        assert_eq!(env_family_of("AUBE_NODE_LINKER"), EnvFamilies::AUBE);
        assert_eq!(env_family_of("npm_config_node_linker"), EnvFamilies::NPM);
        assert_eq!(env_family_of("NPM_CONFIG_NODE_LINKER"), EnvFamilies::NPM);
        assert_eq!(env_family_of("CI"), EnvFamilies::EXTERNAL);

        assert!(EnvFamilies::ALL.contains(EnvFamilies::AUBE));
        assert!(!EnvFamilies::NPM.contains(EnvFamilies::AUBE));
        assert!(
            EnvFamilies::NPM
                .union(EnvFamilies::AUBE)
                .contains(EnvFamilies::AUBE)
        );
        assert!(!EnvFamilies::NONE.contains(EnvFamilies::EXTERNAL));
    }
}
