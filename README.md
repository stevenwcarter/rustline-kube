# rustline-kube

The current Kubernetes context (and namespace) in your tmux status line, as a
[rustline](https://github.com/stevenwcarter/rustline) WASM plugin — a
`kube-tmux`-style widget, but reading directly through rustline's
capability-gated file-read instead of shelling out to `kubectl`.

Reads a kubeconfig (default `<home>/.kube/config`) through rustline's
`allowed_paths` file-read capability: the plugin can only see the exact
paths you've explicitly approved, and the kubeconfig is parsed with a
minimal indentation-aware scan (not a full YAML dependency) that pulls just
`current-context` and its `namespace`.

## Install

    rustline plugin install stevenwcarter/rustline-kube --name kube
    rustline plugin approve kube        # grants: /home/*/.kube/config, /Users/*/.kube/config

(`--name kube` is only needed on rustline v0.1.0, whose default install
name came from the repo; newer builds derive it from the release asset.)

Place it:

    rustline widget enable kube

## Configure

    [plugins.kube.options]
    # path = ""                          # default: <home>/.kube/config
    # format = "󱃾 {context}"             # {context} {namespace} substitute
    # alt_format = "󱃾 {context}:{namespace}"   # click to toggle namespace on/off
    # down_format = ""

`$KUBECONFIG` is invisible to a sandboxed guest — set `path` explicitly if
your kubeconfig lives somewhere other than `~/.kube/config`. A nonstandard
location also needs its own grant:
`rustline plugin path add kube <pattern>`.

If your kubeconfig is a symlink (common with `stow`/dotfiles setups), the
default `resolve_symlinks = false` denies it outright — rustline's symlink
gate refuses any path with a symlink component unless you opt in — and the
failure is invisible in the bar (the widget just renders `down_format`,
empty by default). Either set `resolve_symlinks = true` under
`[plugins.kube]` (and grant the *resolved*, real location the symlink points
at) or point `path` at the real file directly instead of the symlink. A
denial shows up in `rustline plugin denials kube`, not in the bar.

## Building from source

    cargo build --release --target wasm32-unknown-unknown
    cp target/wasm32-unknown-unknown/release/kube.wasm ~/.local/share/rustline/plugins/

The capability manifest (`manifest.toml`) is embedded into the `.wasm` as the
`rustline-manifest` custom section at build time, so `plugin approve` works
on the distributed binary alone. This repo doubles as a worked example of a
standalone (out-of-tree) rustline plugin: scaffolded with `rustline plugin
new`, SDK via a git dependency pinned to a rustline release tag, CI + tag
releases publishing the installable `.wasm`.

## License

MIT
