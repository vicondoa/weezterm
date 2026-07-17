# d2b client seam

WeezTerm carries a configuration seam for canonical d2b v2 workload targets.
It consumes `d2b-client` and `d2b-contracts` directly from the exact d2b source
revision:

```text
4018d9c9652bd826c2e6a9abccdcdcafb832d944
```

The Cargo lockfile binds the same revision. It corresponds to client-toolkit
distribution fingerprint
`c2c99bdd77ba66948fce81161dcc3efde608eefefb96f28fa934c9f58d96d838`.
WeezTerm defines no d2b handshake, frame codec, request or response type, shell
record, error envelope, or target parser.

## Configure a target

Each domain uses the canonical `d2b_client::TargetInput::Workload` type.
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

Configured d2b domains are currently reported as unavailable and are not added
to the mux. WeezTerm does not guess the pending endpoint bootstrap, route
resolution, daemon discovery, session setup, or persistent-shell stream APIs.
There is no legacy public-socket fallback, shell command fallback, SSH mapping,
or direct host-terminal bridge.

Runtime integration can return only after the canonical control and
user-session service APIs are finalized. It must use an explicit Tokio runtime
boundary because WeezTerm's UI and mux use smol; copying protocol or transport
code into this repository is not an acceptable adapter.
