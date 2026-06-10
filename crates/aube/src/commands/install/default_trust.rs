//! The `defaultTrust` build-policy floor.
//!
//! Off by default (`defaultTrust = false`). When enabled, it slots
//! one layer into the dependency build-script precedence chain:
//!
//! 1. explicit `allowBuilds` entry — `true`/`false` always wins, in
//!    both directions (`false` carves a package out of every layer
//!    below, including `dangerouslyAllowAllBuilds`),
//! 2. `dangerouslyAllowAllBuilds` — the allow-all posture,
//! 3. **this floor** — a curated default-trust list, per-package
//!    gated (see below),
//! 4. default deny + the unreviewed-builds warning.
//!
//! Layers 1–2 are [`aube_scripts::BuildPolicy`]; the floor is
//! consulted only on [`aube_scripts::AllowDecision::Unspecified`], so
//! its existence never overrides an explicit decision, and the mere
//! existence of an `allowBuilds` map does not disable it.
//!
//! A listed package ([`aube_scripts::is_default_trusted`], Bun's
//! vendored list) is trusted only when every gate holds:
//!
//! - **registry provenance** — the package resolved from a registry.
//!   git / file / link / tarball sources never qualify, so a malicious
//!   manifest can't borrow a listed name's trust by pointing it at an
//!   arbitrary source (the bun CVE-2026-24910 class).
//! - **OSV coverage** — an OSV `MAL-*` advisory gate ran against this
//!   install's graph (`run_post_resolve_osv_routing` returned true).
//!   Advisory hits abort the install before scripts run, so surviving
//!   packages passed; with every advisory backend off the floor turns
//!   off with them.
//! - **cooling window** — the resolved version's recorded publish time
//!   (the lockfile-graph `time:` data the resolver records whenever
//!   `minimumReleaseAge` is active) is older than the
//!   `minimumReleaseAge` window. Unknown publish time fails closed:
//!   the pnpm `time:` block only persists direct-dep entries, so
//!   transitive deps on a frozen reinstall fall back to deny+warn
//!   rather than being trusted without evidence. `minimumReleaseAge =
//!   0` disables the floor entirely — it consults the cooling defense,
//!   it never substitutes for it.
//!
//! `aube rebuild` does not consult the floor: no advisory gate runs
//! there, and `aube rebuild <name>` already bypasses the policy for
//! explicitly named packages.

use aube_scripts::AllowDecision;
use std::collections::BTreeMap;

/// Resolved floor state for one install. Construct via
/// [`DefaultTrustFloor::from_settings`] after the post-resolve OSV
/// routing has run (its return value is the `osv_gate_active` input),
/// or [`DefaultTrustFloor::disabled`] on paths that must never floor
/// (rebuild).
#[derive(Debug, Clone)]
pub(crate) struct DefaultTrustFloor {
    enabled: bool,
    osv_gate_active: bool,
    /// ISO-8601 UTC cutoff derived from `minimumReleaseAge`: publish
    /// times lexicographically `<=` this string satisfy the window.
    /// `None` when the window is disabled — which disables the floor.
    age_cutoff: Option<String>,
}

impl DefaultTrustFloor {
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            osv_gate_active: false,
            age_cutoff: None,
        }
    }

    /// Resolve the floor from settings. `mra_cli_minutes` is the same
    /// CLI override the resolver's `minimumReleaseAge` receives so the
    /// floor and the resolver agree on the window.
    pub(crate) fn from_settings(
        ctx: &aube_settings::ResolveCtx<'_>,
        mra_cli_minutes: Option<u64>,
        osv_gate_active: bool,
    ) -> Self {
        let enabled = aube_settings::resolved::default_trust(ctx);
        if !enabled {
            return Self::disabled();
        }
        let minutes =
            mra_cli_minutes.unwrap_or_else(|| aube_settings::resolved::minimum_release_age(ctx));
        let age_cutoff = aube_resolver::MinimumReleaseAge {
            minutes,
            exclude: Default::default(),
            strict: false,
        }
        .cutoff();
        Self {
            enabled,
            osv_gate_active,
            age_cutoff,
        }
    }

    /// Cheap pre-check: could this floor trust *anything*? Lets the
    /// dep-script phase keep its "no allow rules → skip entirely"
    /// fast path when the floor can't fire anyway.
    pub(crate) fn may_allow_any(&self) -> bool {
        self.enabled && self.osv_gate_active && self.age_cutoff.is_some()
    }

    /// Whether the floor trusts this resolved package. `times` is the
    /// graph's publish-time map (`LockfileGraph::times`).
    pub(crate) fn trusts(
        &self,
        pkg: &aube_lockfile::LockedPackage,
        times: &BTreeMap<String, String>,
    ) -> bool {
        let Some(cutoff) = self.age_cutoff.as_deref() else {
            return false;
        };
        if !self.enabled || !self.osv_gate_active {
            return false;
        }
        // Registry-resolved only. `local_source` covers file / link /
        // tarball / git / portal / exec; `registry_git_hosted` covers
        // registry-keyed entries third-party lockfiles mark as hosted
        // git. Match on `registry_name()` so an npm alias can't borrow
        // a listed name's trust (same rule as `BuildPolicy::decide`).
        if pkg.local_source.is_some() || pkg.registry_git_hosted {
            return false;
        }
        if !aube_scripts::is_default_trusted(pkg.registry_name()) {
            return false;
        }
        // Cooling window, fail-closed on unknown publish time. The
        // resolver keys fresh entries by dep_path (peer suffix
        // included); the pnpm lockfile round-trip re-keys them as
        // canonical `name@version`. Probe all spellings.
        let canonical = format!("{}@{}", pkg.registry_name(), pkg.version);
        let aliased = format!("{}@{}", pkg.name, pkg.version);
        let published = times
            .get(&canonical)
            .or_else(|| times.get(&aliased))
            .or_else(|| times.get(&pkg.dep_path));
        match published {
            Some(t) => t.as_str() <= cutoff,
            None => false,
        }
    }
}

/// The full precedence chain for one package: explicit policy first
/// (allows, denies, `dangerouslyAllowAllBuilds`), the floor on
/// `Unspecified`. Returns `Unspecified` when neither layer decides —
/// the caller's default-deny + unreviewed warning applies.
pub(crate) fn decide_with_floor(
    policy: &aube_scripts::BuildPolicy,
    floor: &DefaultTrustFloor,
    pkg: &aube_lockfile::LockedPackage,
    times: &BTreeMap<String, String>,
) -> AllowDecision {
    match policy.decide(pkg.registry_name(), &pkg.version) {
        AllowDecision::Unspecified if floor.trusts(pkg, times) => AllowDecision::Allow,
        decision => decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aube_manifest::AllowBuildRaw;

    fn active_floor() -> DefaultTrustFloor {
        DefaultTrustFloor {
            enabled: true,
            osv_gate_active: true,
            age_cutoff: aube_resolver::MinimumReleaseAge {
                minutes: 1440,
                exclude: Default::default(),
                strict: false,
            }
            .cutoff(),
        }
    }

    /// `esbuild` is on the vendored list; pinned by the aube-scripts
    /// loader test, relied on here.
    fn listed_pkg() -> aube_lockfile::LockedPackage {
        aube_lockfile::LockedPackage {
            name: "esbuild".into(),
            version: "0.19.0".into(),
            dep_path: "esbuild@0.19.0".into(),
            ..Default::default()
        }
    }

    /// Publish-time map placing `pkg` `minutes` in the past, formatted
    /// exactly like the registry `time:` data the floor compares
    /// against. Reuses `MinimumReleaseAge::cutoff` ("now − minutes" as
    /// an ISO-8601 string) so the comparison shape matches production.
    fn times_published_minutes_ago(
        pkg: &aube_lockfile::LockedPackage,
        minutes: u64,
    ) -> BTreeMap<String, String> {
        let published = aube_resolver::MinimumReleaseAge {
            minutes,
            exclude: Default::default(),
            strict: false,
        }
        .cutoff()
        .expect("non-zero minutes always produce a cutoff");
        let mut times = BTreeMap::new();
        times.insert(format!("{}@{}", pkg.name, pkg.version), published);
        times
    }

    fn policy_from(pairs: &[(&str, bool)], dangerously: bool) -> aube_scripts::BuildPolicy {
        let map: BTreeMap<String, AllowBuildRaw> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), AllowBuildRaw::Bool(*v)))
            .collect();
        let (p, errs) = aube_scripts::BuildPolicy::from_config(&map, &[], &[], dangerously);
        assert!(errs.is_empty(), "unexpected policy warnings: {errs:?}");
        p
    }

    #[test]
    fn floor_allows_an_aged_registry_package_the_policy_left_unspecified() {
        let pkg = listed_pkg();
        let times = times_published_minutes_ago(&pkg, 10 * 1440);
        let decision = decide_with_floor(&policy_from(&[], false), &active_floor(), &pkg, &times);
        assert_eq!(decision, AllowDecision::Allow);
    }

    #[test]
    fn explicit_false_carves_a_package_out_of_the_floor() {
        let pkg = listed_pkg();
        let times = times_published_minutes_ago(&pkg, 10 * 1440);
        let policy = policy_from(&[("esbuild", false)], false);
        assert_eq!(
            decide_with_floor(&policy, &active_floor(), &pkg, &times),
            AllowDecision::Deny,
            "an explicit allowBuilds false must beat the floor"
        );
    }

    #[test]
    fn allow_builds_existence_does_not_disable_the_floor() {
        // The map naming an unrelated package must not flip the floor
        // off for everything else — approve-builds writes the map on
        // first approval, and existence-toggling would revoke
        // unrelated packages' trust.
        let pkg = listed_pkg();
        let times = times_published_minutes_ago(&pkg, 10 * 1440);
        let policy = policy_from(&[("some-other-pkg", false)], false);
        assert_eq!(
            decide_with_floor(&policy, &active_floor(), &pkg, &times),
            AllowDecision::Allow
        );
    }

    #[test]
    fn floor_ignores_git_file_and_tarball_sources() {
        let times = times_published_minutes_ago(&listed_pkg(), 10 * 1440);
        let floor = active_floor();
        let mut git = listed_pkg();
        git.local_source = Some(aube_lockfile::LocalSource::Git(aube_lockfile::GitSource {
            url: "https://github.com/evil/esbuild.git".into(),
            committish: None,
            resolved: "0123456789012345678901234567890123456789".into(),
            integrity: None,
            subpath: None,
        }));
        assert!(!floor.trusts(&git, &times), "git source must never floor");
        let mut tarball = listed_pkg();
        tarball.local_source = Some(aube_lockfile::LocalSource::Tarball("./esbuild.tgz".into()));
        assert!(
            !floor.trusts(&tarball, &times),
            "file tarball must never floor"
        );
        let mut hosted = listed_pkg();
        hosted.registry_git_hosted = true;
        assert!(
            !floor.trusts(&hosted, &times),
            "registry-keyed hosted git must never floor"
        );
    }

    #[test]
    fn floor_refuses_an_alias_borrowing_a_listed_name() {
        let times = times_published_minutes_ago(&listed_pkg(), 10 * 1440);
        let mut aliased = listed_pkg();
        aliased.name = "esbuild".into();
        aliased.alias_of = Some("evil-pkg".into());
        assert!(
            !active_floor().trusts(&aliased, &times),
            "trust must key off the registry name, not the in-tree alias"
        );
    }

    #[test]
    fn floor_defers_to_the_osv_gate_and_the_off_switch() {
        let pkg = listed_pkg();
        let times = times_published_minutes_ago(&pkg, 10 * 1440);
        let mut no_osv = active_floor();
        no_osv.osv_gate_active = false;
        assert!(!no_osv.trusts(&pkg, &times), "no OSV coverage → no floor");
        let off = DefaultTrustFloor::disabled();
        assert!(!off.trusts(&pkg, &times), "defaultTrust=false → no floor");
        assert!(!off.may_allow_any());
    }

    #[test]
    fn floor_requires_the_cooling_window_per_version() {
        let pkg = listed_pkg();
        let floor = active_floor();
        let young = times_published_minutes_ago(&pkg, 60);
        assert!(
            !floor.trusts(&pkg, &young),
            "a version younger than minimumReleaseAge must not floor"
        );
        assert!(
            !floor.trusts(&pkg, &BTreeMap::new()),
            "unknown publish time must fail closed"
        );
        let mut no_window = active_floor();
        no_window.age_cutoff = None;
        let aged = times_published_minutes_ago(&pkg, 10 * 1440);
        assert!(
            !no_window.trusts(&pkg, &aged),
            "minimumReleaseAge=0 must disable the floor"
        );
    }

    #[test]
    fn explicit_false_survives_dangerously_allow_all_when_the_floor_is_active() {
        // The documented chain when defaultTrust is on: explicit entry
        // > allow-all > floor. The install path composes this via
        // `allow_all_except_denied`; pin the end-to-end decision here.
        let pkg = listed_pkg();
        let times = times_published_minutes_ago(&pkg, 10 * 1440);
        let policy = policy_from(&[("esbuild", false)], false).allow_all_except_denied();
        assert_eq!(
            decide_with_floor(&policy, &active_floor(), &pkg, &times),
            AllowDecision::Deny
        );
        let other = aube_lockfile::LockedPackage {
            name: "anything-else".into(),
            version: "1.0.0".into(),
            dep_path: "anything-else@1.0.0".into(),
            ..Default::default()
        };
        assert_eq!(
            decide_with_floor(&policy, &active_floor(), &other, &times),
            AllowDecision::Allow,
            "allow-all still wins above the floor for everything not denied"
        );
    }
}
