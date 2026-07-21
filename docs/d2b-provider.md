# d2b client seam

WeezTerm carries a configuration seam for canonical d2b v2 workload targets.
It consumes the `d2b-client-toolkit` 2.0.0 facade from the exact distribution
revision:

```text
926de54e7320599c373524a10b65aaf13b6ff422
```

That distribution re-exports canonical d2b source revision
`9dc902243cdd7aba7ef269988b96f0aae6e037da`, with source fingerprint
`5a20cef3a64281df819eeb76bdfe385999755479b467b559653011582fb9c043`.
The Cargo lockfile binds both revisions through the facade. WeezTerm defines no
d2b handshake, frame codec, request or response type, shell record, error
envelope, or target parser.

`nix build .#source` pins the same two revisions as explicit `flake = false`
inputs (`d2b`, `d2b-toolkit`) in `flake.nix`/`nix/flake.nix`. Their `narHash`
values (captured in `flake.lock`) are threaded into
`sourcePackage.cargoLock.outputHashes` for the `d2b-client`, `d2b-contracts`,
`d2b-session`, and `d2b-client-toolkit` crates, so vendoring those four
crates goes through the hermetic `fetchgit` fixed-output-derivation path
instead of `cargoLock.allowBuiltinFetchGit`'s impure fallback.
`allowBuiltinFetchGit` remains set only for the seam's other, unrelated git
dependencies (`xcb-imdkit`, `finl_unicode`). Bumping either revision requires
updating the `Cargo.toml`/`Cargo.lock` git rev *and* the matching flake input
URL, then re-running `nix flake lock` (both the root and `nix/` lockfiles) to
refresh the pinned `narHash`.

The toolkit flake package is named `d2b-client-toolkit`. Its immutable source
layout is:

```text
share/d2b-client-toolkit/
├── distribution/
└── d2b/
```

There is no `d2b-toolkit` package, crate, or share path in the current seam.

## Configure a target

Each domain uses the canonical
`d2b_client_toolkit::TargetInput::Workload` type.
Configuration supplies the exact canonical realm and workload IDs:

```lua
return {
  d2b_domains = {
    {
      name = "work",
      target = {
        realm_id = "aaaaaaaaaaaaaaaaaaaa",
        workload_id = "bbbbbbbbbbbbbbbbbbbq",
      },
    },
  },
}
```

Both IDs use d2b's 20-character lowercase unpadded base32 short-ID grammar.
Invalid IDs fail configuration without echoing the submitted identity; the
error names the configured `name` (the WeezTerm domain), not the rejected
`realm_id`/`workload_id` value, so a multi-domain config still points at the
right `[[d2b_domains]]` entry.
String targets, VM aliases, and direct socket-path overrides are not accepted.

## Runtime boundary

The configuration seam derives a canonical
`TargetInput::Service { service: ServiceKind::Shell, ... }` from each workload
target. The facade also exposes the frozen typed `DaemonClient`, guest terminal
client, and generated user/desktop service clients without local wrappers.
Configured d2b domains are currently reported as unavailable and are not added
to the mux because canonical endpoint acquisition and workload-to-shell routing
are not integrated. WeezTerm does not infer a local VM name, read root-owned
state, or connect to a caller-selected socket. There is no legacy public-socket
fallback, shell command fallback, SSH mapping, or direct host-terminal bridge.

Runtime integration must consume canonical client routing and terminal streams
through an explicit Tokio runtime boundary because WeezTerm's UI and mux use
smol. Copying protocol or transport code into this repository is not an
acceptable adapter, and missing integrated routing remains a fail-closed
condition.
