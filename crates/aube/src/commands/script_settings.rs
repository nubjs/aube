use std::path::Path;

use miette::{Context, IntoDiagnostic};

use super::FileSources;

pub(crate) fn configure_script_settings(ctx: &aube_settings::ResolveCtx<'_>) {
    let node_options = aube_settings::resolved::node_options(ctx).and_then(non_empty_string);
    let script_shell = aube_settings::resolved::script_shell(ctx)
        .and_then(|s| non_empty_string(s).map(Into::into));
    let unsafe_perm = aube_settings::resolved::unsafe_perm(ctx);
    let shell_emulator = aube_settings::resolved::shell_emulator(ctx);
    // Carry the embedder-owned overlay forward: an embedder (e.g. nub)
    // installs its `env_overlay` / `path_prepends` once up front, and this
    // settings pass — which runs later, inside the install command — must not
    // wipe them. The `.npmrc`/workspace-derived fields above are the only ones
    // this function owns.
    let prior = aube_scripts::script_settings_snapshot();
    // Runtime switching: `crate::runtime::ensure` must have run before
    // this for lifecycle scripts to see the pinned node — the install
    // driver resolves the runtime early, then configures script
    // settings. When no context exists (or no switching is active)
    // these stay `None` and scripts inherit PATH untouched. Under nub the
    // runtime resolver is gated off (see `runtime::set_runtime_switching_enabled`),
    // so `current()` yields the PATH-fallback context and both fields are `None`.
    let runtime = crate::runtime::current();
    aube_scripts::set_script_settings(aube_scripts::ScriptSettings {
        node_options,
        script_shell,
        unsafe_perm,
        shell_emulator,
        env_overlay: prior.env_overlay,
        path_prepends: prior.path_prepends,
        node_bin_dir: runtime.and_then(|r| r.bin_dir.clone()),
        node_exe: runtime.and_then(|r| r.node_bin.clone()),
    });
}

/// Load `.npmrc` + workspace settings for `cwd` and push them into the
/// process-wide script settings snapshot. Used by commands that run
/// lifecycle hooks (pack/publish/version) outside the install path,
/// which already does this via `configure_script_settings` directly.
pub(crate) fn configure_script_settings_for_cwd(cwd: &Path) -> miette::Result<()> {
    let files = FileSources::load(cwd);
    let (_, raw_workspace) = aube_manifest::workspace::load_both(cwd)
        .into_diagnostic()
        .wrap_err("failed to load workspace config")?;
    let env_snapshot = aube_settings::values::capture_env();
    let ctx = files.ctx(&raw_workspace, &env_snapshot, &[]);
    configure_script_settings(&ctx);
    Ok(())
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
