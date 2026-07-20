# d2b client seam

WeezTerm carries a configuration seam for canonical d2b v2 workload targets.
It consumes the `d2b-client-toolkit` 2.0.0 facade from the exact distribution
revision:

```text
800c2878533f600d8f085b3d2aafcddb970232b2
```

That distribution re-exports canonical d2b source revision
`4018d9c9652bd826c2e6a9abccdcdcafb832d944`, with source fingerprint
`c2c99bdd77ba66948fce81161dcc3efde608eefefb96f28fa934c9f58d96d838`.
The Cargo lockfile binds both revisions through the facade. WeezTerm defines no
d2b handshake, frame codec, request or response type, shell record, error
envelope, or target parser.

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
Invalid IDs fail configuration without echoing the submitted identity.
String targets, VM aliases, and direct socket-path overrides are not accepted.

## Runtime boundary

The configuration seam derives a canonical
`TargetInput::Service { service: ServiceKind::Shell, ... }` from each workload
target. Configured d2b domains are currently reported as unavailable and are
not added to the mux because workload-to-shell routing is not integrated.
WeezTerm does not infer a local VM name, read root-owned state, or connect to a
caller-selected socket. There is no legacy public-socket fallback, shell
command fallback, SSH mapping, or direct host-terminal bridge.

Runtime integration must consume canonical client routing and terminal streams
through an explicit Tokio runtime boundary because WeezTerm's UI and mux use
smol. Copying protocol or transport code into this repository is not an
acceptable adapter, and missing integrated routing remains a fail-closed
condition.
