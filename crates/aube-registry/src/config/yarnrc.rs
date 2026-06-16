use base64::Engine as _;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::url::{normalize_registry_url, registry_uri_key};

#[derive(Default)]
pub(super) struct SplitYarnrcEntries {
    pub user: Vec<(String, String)>,
    pub project: Vec<(String, String)>,
}

pub(super) fn load_yarnrc_entries_split(
    home: Option<&Path>,
    project_dir: &Path,
) -> SplitYarnrcEntries {
    if !aube_util::engine_context().read_yarn_config {
        return SplitYarnrcEntries::default();
    }
    load_yarnrc_entries_split_with_home(home, project_dir)
}

pub(super) fn load_yarnrc_entries_split_with_home(
    home: Option<&Path>,
    starting_dir: &Path,
) -> SplitYarnrcEntries {
    let mut out = SplitYarnrcEntries::default();
    out.user = load_user_yarnrc_entries_with_home(home);
    out.project = load_project_yarnrc_entries_with_home(home, starting_dir);
    out
}

pub(super) fn load_user_yarnrc_entries(home: Option<&Path>) -> Vec<(String, String)> {
    if !aube_util::engine_context().read_yarn_config {
        return Vec::new();
    }
    load_user_yarnrc_entries_with_home(home)
}

fn load_user_yarnrc_entries_with_home(home: Option<&Path>) -> Vec<(String, String)> {
    let Some(home) = home else {
        return Vec::new();
    };
    load_yarnrc_entries_from_path(&home.join(".yarnrc.yml"))
}

pub(super) fn load_project_yarnrc_entries(starting_dir: &Path) -> Vec<(String, String)> {
    if !aube_util::engine_context().read_yarn_config {
        return Vec::new();
    }
    load_project_yarnrc_entries_with_home(aube_util::env::home_dir().as_deref(), starting_dir)
}

fn load_project_yarnrc_entries_with_home(
    home: Option<&Path>,
    starting_dir: &Path,
) -> Vec<(String, String)> {
    yarnrc_paths_from_root(starting_dir)
        .into_iter()
        .filter(|path| !home.is_some_and(|home| path == &home.join(".yarnrc.yml")))
        .flat_map(|path| load_yarnrc_entries_from_path(&path))
        .collect()
}

pub(super) fn yarn_env_entries_from(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut config = YarnRc::default();
    for (key, value) in env {
        match yarn_env_key(key).as_deref() {
            Some("npmRegistryServer") => config.npm_registry_server = Some(value.clone()),
            Some("npmAuthToken") => config.npm_auth_token = Some(value.clone()),
            Some("npmAuthIdent") => config.npm_auth_ident = Some(value.clone()),
            Some("nodeLinker") => config.node_linker = Some(value.clone()),
            _ => {}
        }
    }
    config.into_entries()
}

pub(super) fn yarn_env_entries_from_std() -> Vec<(String, String)> {
    let env: Vec<(String, String)> = std::env::vars().collect();
    yarn_env_entries_from(&env)
}

fn yarnrc_paths_from_root(starting_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = starting_dir.to_path_buf();
    loop {
        dirs.push(current.clone());
        if !current.pop() {
            break;
        }
    }
    dirs.reverse();
    dirs.into_iter()
        .map(|dir| dir.join(".yarnrc.yml"))
        .filter(|path| path.is_file())
        .collect()
}

fn load_yarnrc_entries_from_path(path: &Path) -> Vec<(String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    translate_yarnrc_content(&content)
}

pub(super) fn translate_yarnrc_content(content: &str) -> Vec<(String, String)> {
    let Ok(config) = aube_manifest::parse_yaml::<YarnRc>(Path::new(".yarnrc.yml"), content.into())
    else {
        return Vec::new();
    };
    config.into_entries()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct YarnRc {
    npm_registry_server: Option<String>,
    npm_auth_token: Option<String>,
    npm_auth_ident: Option<String>,
    node_linker: Option<String>,
    #[serde(default)]
    npm_scopes: BTreeMap<String, YarnScope>,
    #[serde(default)]
    npm_registries: BTreeMap<String, YarnRegistry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct YarnScope {
    npm_registry_server: Option<String>,
    npm_auth_token: Option<String>,
    npm_auth_ident: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct YarnRegistry {
    npm_auth_token: Option<String>,
    npm_auth_ident: Option<String>,
}

impl YarnRc {
    fn into_entries(self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let default_registry = self
            .npm_registry_server
            .as_deref()
            .map(normalize_registry_url);
        let registry_configs = self
            .npm_registries
            .iter()
            .map(|(registry, _)| normalize_registry_url(registry))
            .collect::<BTreeSet<_>>();
        let scope_registry_counts = scope_registry_counts(&self.npm_scopes);

        if let Some(registry) = &default_registry {
            push(&mut out, "registry", registry.clone());
        }
        push_auth(
            &mut out,
            default_registry.as_deref(),
            self.npm_auth_token.as_deref(),
            self.npm_auth_ident.as_deref(),
        );

        for (registry, config) in self.npm_registries {
            let registry = normalize_registry_url(&registry);
            push_auth(
                &mut out,
                Some(&registry),
                config.npm_auth_token.as_deref(),
                config.npm_auth_ident.as_deref(),
            );
        }

        for (scope, config) in self.npm_scopes {
            let scope = if scope.starts_with('@') {
                scope
            } else {
                format!("@{scope}")
            };
            let explicit_registry = config
                .npm_registry_server
                .as_deref()
                .map(normalize_registry_url);
            let registry = explicit_registry
                .clone()
                .or_else(|| default_registry.clone());
            if let Some(registry) = &registry {
                push(&mut out, format!("{scope}:registry"), registry.clone());
            }
            let scope_auth_is_representable = explicit_registry.as_ref().is_some_and(|registry| {
                Some(registry) != default_registry.as_ref()
                    && !registry_configs.contains(registry)
                    && scope_registry_counts.get(registry).copied().unwrap_or(0) == 1
            });
            // Yarn auth can be package-scope-specific. The existing registry
            // model cannot represent that, so only translate scope auth when
            // the scope owns a unique custom registry. Otherwise translating it
            // would widen the credential to every package fetched from the same
            // registry.
            if scope_auth_is_representable {
                push_auth(
                    &mut out,
                    registry.as_deref(),
                    config.npm_auth_token.as_deref(),
                    config.npm_auth_ident.as_deref(),
                );
            }
        }

        if let Some(linker) = self.node_linker.as_deref().map(str::trim) {
            match linker.to_ascii_lowercase().as_str() {
                "node-modules" => push(&mut out, "nodeLinker", "hoisted"),
                "pnpm" => push(&mut out, "nodeLinker", "isolated"),
                // PnP generation is out of scope. Leave it to nub's existing
                // Yarn-PnP warning/refusal path instead of pretending support.
                _ => {}
            }
        }

        out
    }
}

fn push(out: &mut Vec<(String, String)>, key: impl Into<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty() {
        out.push((key.into(), value));
    }
}

fn push_auth(
    out: &mut Vec<(String, String)>,
    registry: Option<&str>,
    token: Option<&str>,
    ident: Option<&str>,
) {
    let Some(registry) = registry else {
        return;
    };
    let uri = registry_uri_key(registry);
    if let Some(token) = token.filter(|v| !v.trim().is_empty()) {
        push(out, format!("{uri}:_authToken"), token);
    }
    if let Some(ident) = ident.filter(|v| !v.trim().is_empty()) {
        push(
            out,
            format!("{uri}:_auth"),
            yarn_auth_ident_to_npm_auth(ident),
        );
    }
}

fn yarn_auth_ident_to_npm_auth(ident: &str) -> String {
    if ident.contains(':') {
        base64::engine::general_purpose::STANDARD.encode(ident)
    } else {
        ident.to_string()
    }
}

fn scope_registry_counts(scopes: &BTreeMap<String, YarnScope>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for scope in scopes.values() {
        if let Some(registry) = scope
            .npm_registry_server
            .as_deref()
            .map(normalize_registry_url)
        {
            *counts.entry(registry).or_insert(0) += 1;
        }
    }
    counts
}

fn yarn_env_key(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    let rest = lower.strip_prefix("yarn_")?;
    match rest {
        "npm_registry_server" => Some("npmRegistryServer".to_string()),
        "npm_auth_token" => Some("npmAuthToken".to_string()),
        "npm_auth_ident" => Some("npmAuthIdent".to_string()),
        "node_linker" => Some("nodeLinker".to_string()),
        _ => None,
    }
}
