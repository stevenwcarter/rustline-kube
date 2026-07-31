//! `kube` — a rustline widget showing the current Kubernetes context (and
//! its namespace) from a kubeconfig, read through the host's capability-gated
//! file-read. The kubeconfig is YAML, but only two fields matter here, so a
//! minimal indentation-aware scan replaces a full YAML dependency
//! (serde_yaml is unmaintained; a statusline widget doesn't need it).
//!
//! Pure logic lives here and is unit-tested on the host target; the Extism
//! guest glue below only compiles for wasm32.

use serde::Deserialize;

/// This plugin's `[plugins.kube.options]` table.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Kubeconfig path. Empty (the default) derives `<home>/.kube/config`
    /// from the render context — the guest can't read `$KUBECONFIG`.
    pub path: String,
    /// Render format. `{context}` and `{namespace}` substitute.
    pub format: String,
    /// Click-toggle alternate view.
    pub alt_format: String,
    /// Shown when no kubeconfig/current-context exists. Empty renders nothing.
    pub down_format: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            path: String::new(),
            format: "\u{f10fe} {context}".to_string(), // nf-md-kubernetes + space
            alt_format: String::new(),
            down_format: String::new(),
        }
    }
}

/// A parsed kubeconfig: the active context name and its namespace.
#[derive(Debug, PartialEq, Eq)]
pub struct KubeInfo {
    pub context: String,
    pub namespace: String,
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// `key: value` on one (trimmed) line -> the unquoted value, else None.
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(key)?.strip_prefix(':')?;
    let v = strip_quotes(rest);
    (!v.is_empty()).then_some(v)
}

/// Extract the current context + its namespace. `None` when there is no
/// top-level `current-context:` (an empty or foreign file). A context whose
/// entry carries no `namespace:` — or is missing from `contexts:` — reports
/// namespace `"default"`, matching kubectl's own behavior.
pub fn parse_kubeconfig(text: &str) -> Option<KubeInfo> {
    let current = text
        .lines()
        .filter(|l| !l.starts_with([' ', '\t']))
        .find_map(|l| value_of(l, "current-context"))?
        .to_string();
    let namespace = context_namespace(text, &current)
        .unwrap_or("default")
        .to_string();
    Some(KubeInfo {
        context: current,
        namespace,
    })
}

/// The `namespace:` recorded for the context named `current`, scanning only
/// the top-level `contexts:` section — the `clusters:`/`users:` lists carry
/// `name:` keys too and must not be consulted. Handles both kubectl's
/// column-0 list items (`- context:` at the left margin) and indented ones.
fn context_namespace<'a>(text: &'a str, current: &str) -> Option<&'a str> {
    let mut in_contexts = false;
    let mut name: Option<&str> = None;
    let mut ns: Option<&str> = None;
    for line in text.lines() {
        let is_top_level_key =
            !line.trim().is_empty() && !line.starts_with([' ', '\t']) && !line.starts_with('-');
        if is_top_level_key {
            if in_contexts && name == Some(current) {
                return ns;
            }
            in_contexts = line.trim_end() == "contexts:";
            (name, ns) = (None, None);
            continue;
        }
        if !in_contexts {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") || trimmed == "-" {
            // New list item: flush the previous one.
            if name == Some(current) {
                return ns;
            }
            (name, ns) = (None, None);
        }
        let item_line = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        if let Some(v) = value_of(item_line, "name") {
            name = Some(v);
        }
        if let Some(v) = value_of(item_line, "namespace") {
            ns = Some(v);
        }
    }
    if in_contexts && name == Some(current) {
        ns
    } else {
        None
    }
}

/// Substitute `{context}`/`{namespace}`; unknown placeholders pass through.
pub fn render_format(format: &str, info: &KubeInfo) -> String {
    format
        .replace("{context}", &info.context)
        .replace("{namespace}", &info.namespace)
}

/// The capability manifest, embedded as the `rustline-manifest` custom
/// section so `rustline plugin approve kube` can read the requests straight
/// out of the distributed `.wasm` — `plugin install` downloads only this
/// binary, so a sidecar file would never reach the user's plugin dir.
#[cfg(target_arch = "wasm32")]
#[used]
#[unsafe(link_section = "rustline-manifest")]
pub static MANIFEST: [u8; include_bytes!("../manifest.toml").len()] =
    *include_bytes!("../manifest.toml");

#[cfg(target_arch = "wasm32")]
mod guest {
    use super::*;
    use rustline_plugin_sdk::{
        GuestRender, LogLevel, Segment, active_format, export_plugin, file_read, log,
    };

    fn render(input: &GuestRender) -> Vec<Segment> {
        let opts: Options = serde_json::from_value(input.config.clone()).unwrap_or_default();
        let path = if !opts.path.is_empty() {
            opts.path.clone()
        } else if !input.context.home.is_empty() {
            format!("{}/.kube/config", input.context.home)
        } else {
            log(
                LogLevel::Warn,
                "kube: no `path` option and no home dir in context",
            );
            return down_segment(&opts.down_format);
        };

        match file_read(&path) {
            Ok(r) if r.ok && r.exists => match parse_kubeconfig(&r.contents) {
                Some(info) => {
                    let format =
                        active_format(&input.context, "kube", &opts.format, &opts.alt_format);
                    vec![Segment::new(render_format(format, &info))]
                }
                None => {
                    log(
                        LogLevel::Info,
                        &format!("kube: no current-context in {path}"),
                    );
                    down_segment(&opts.down_format)
                }
            },
            Ok(r) if r.ok => {
                log(LogLevel::Info, &format!("kube: {path} does not exist"));
                down_segment(&opts.down_format)
            }
            Ok(r) => {
                log(
                    LogLevel::Warn,
                    &format!("kube: read denied/failed: {}", r.error),
                );
                down_segment(&opts.down_format)
            }
            Err(e) => {
                log(LogLevel::Warn, &format!("kube: host call failed: {e}"));
                down_segment(&opts.down_format)
            }
        }
    }

    fn down_segment(down_format: &str) -> Vec<Segment> {
        if down_format.is_empty() {
            Vec::new()
        } else {
            vec![Segment::new(down_format.to_string())]
        }
    }

    export_plugin!(name: "kube", render: render);
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUBECTL_STYLE: &str = "\
apiVersion: v1
clusters:
- cluster:
    server: https://prod.example:6443
  name: prod-cluster
contexts:
- context:
    cluster: prod-cluster
    namespace: monitoring
    user: admin
  name: prod
- context:
    cluster: dev-cluster
    user: dev
  name: dev
current-context: prod
kind: Config
users:
- name: admin
  user: {}
";

    #[test]
    fn parses_kubectl_style_column_zero_items() {
        let info = parse_kubeconfig(KUBECTL_STYLE).unwrap();
        assert_eq!(info.context, "prod");
        assert_eq!(info.namespace, "monitoring");
    }

    #[test]
    fn context_without_namespace_reports_default() {
        let text = KUBECTL_STYLE.replace("current-context: prod", "current-context: dev");
        let info = parse_kubeconfig(&text).unwrap();
        assert_eq!(info.context, "dev");
        assert_eq!(info.namespace, "default");
    }

    #[test]
    fn current_context_missing_from_contexts_list_reports_default() {
        let text = KUBECTL_STYLE.replace("current-context: prod", "current-context: gone");
        assert_eq!(parse_kubeconfig(&text).unwrap().namespace, "default");
    }

    #[test]
    fn clusters_and_users_sections_never_match() {
        // A cluster named like the current context must not donate fields.
        let text = "\
clusters:
- cluster:
    namespace: wrong
  name: prod
contexts:
- context:
    cluster: c
    namespace: right
  name: prod
current-context: prod
";
        assert_eq!(parse_kubeconfig(text).unwrap().namespace, "right");
    }

    #[test]
    fn quoted_values_are_unquoted() {
        let text = "current-context: \"prod\"\ncontexts:\n- context:\n    namespace: 'ns1'\n  name: \"prod\"\n";
        let info = parse_kubeconfig(text).unwrap();
        assert_eq!(info.context, "prod");
        assert_eq!(info.namespace, "ns1");
    }

    #[test]
    fn indented_list_items_also_parse() {
        let text = "\
current-context: prod
contexts:
  - context:
      namespace: ns2
    name: prod
";
        assert_eq!(parse_kubeconfig(text).unwrap().namespace, "ns2");
    }

    #[test]
    fn no_current_context_is_none() {
        assert_eq!(parse_kubeconfig(""), None);
        assert_eq!(parse_kubeconfig("apiVersion: v1\n"), None);
    }

    #[test]
    fn render_format_substitutes_both() {
        let info = KubeInfo {
            context: "prod".into(),
            namespace: "mon".into(),
        };
        assert_eq!(render_format("{context}:{namespace}", &info), "prod:mon");
    }
}
