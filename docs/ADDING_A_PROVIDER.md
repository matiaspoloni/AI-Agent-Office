# Adding a Provider

A provider is any coding-agent CLI Agent Office can detect, launch or observe. The
core, the database and the UI are provider-agnostic, so adding one should mostly
mean adding a crate under `crates/providers/`.

## 1. Research first

Before writing code, add a section to [PROVIDER_CAPABILITIES.md](PROVIDER_CAPABILITIES.md):

* Which **official** mechanisms exist (hooks, JSON output, JSON-RPC/ACP server,
  SDK, documented logs)? Prefer, in this order: a structured protocol on stdio →
  hooks → documented JSON output. Never scrape a TUI when anything structured exists.
* Fill a row for every capability, for managed and external sessions, with
  `Supported` / `Partial` / `Experimental` / `Runtime` / `Unsupported`.
* Record the version you inspected and how you verified it.

If a capability does not exist, it stays `Unsupported`. Do not simulate it.

## 2. Create the crate

```
crates/providers/ao-provider-<name>/
├─ Cargo.toml          # depends on ao-core (+ ao-detect, ao-process, ao-ipc as needed)
├─ src/lib.rs          # the adapter
├─ src/mapping.rs      # provider payload → AgentEvent (pure functions)
└─ tests/              # mapping tests with fixtures, protocol tests with a fake binary
```

Add the crate to the workspace `members` in the root `Cargo.toml`.

## 3. Implement `ProviderAdapter`

```rust
pub struct MyAdapter { /* config, detected paths */ }

#[async_trait]
impl ProviderAdapter for MyAdapter {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("my-provider"),
            display_name: "My Provider".into(),
            accent_color: "#7c3aed".into(),   // used to tint the character
            badge: "MP".into(),               // 1–3 characters shown on the badge
            executable_names: vec!["my-agent".into()],
            homepage: Some("https://example.com".into()),
            simulated: false,
        }
    }

    fn capabilities(&self) -> CapabilityProfile { /* honest values from the doc */ }

    async fn detect_installation(&self) -> InstallationInfo {
        // PATH first, then the provider's documented install folders.
        ao_detect::detect(&self.descriptor().executable_names, EXTRA_DIRS, &["--version"]).await
    }
    // Implement only what the provider really supports.
    // Everything else keeps the default: Err(ProviderError::Unsupported { .. }).
}
```

Rules:

* **Push, don't pull.** Emit normalized events through the `EventSink` in
  `AdapterContext`. Never talk to the UI directly.
* **Map in pure functions** (the Claude adapter uses `hooks.rs` and `stream.rs`) so
  they can be tested with fixtures.
* **Tolerate unknown input.** Unknown fields/variants are ignored; malformed payloads
  are logged (redacted) and dropped. An adapter must never panic on provider input.
* **Never kill foreign processes.** Only processes started through `ao-process` in
  the adapter's own session can be stopped.
* **Normalize tool categories** using the table in PROVIDER_CAPABILITIES.md §6.
* **Usage/cost:** only report numbers the provider gives you. Mark estimates.

## 4. Register it

Add one line in `src-tauri/src/providers.rs`:

```rust
registry.register(Arc::new(ao_provider_my::MyAdapter::new()));
```

That's all the wiring: Diagnostics, Command Center, the New Agent dialog and the
office pick the provider up from its descriptor and capabilities.

## 5. Hooks (if the provider has them)

The relay is shared: `AdapterContext::relay` is a `RelayCommand`, and
`relay.args("<provider-id>", "global" | "managed", wait_secs)` gives the argument
list for one hook entry (`agent-office.exe hook <provider-id> --origin …`). Write it
in the provider's config in **exec form** (program + args, no shell).

* `handle_hook(call, ctx)` receives the parsed stdin JSON (`call.payload`), whether it
  came from a session you launched (`call.managed_origin`) and the relay timestamp
  (use it as the event time). Emit events through `ctx.sink`; return a `HookReply`
  whose `stdout` is printed back to the CLI (e.g. a permission decision). An empty
  reply means "carry on as if Agent Office did not exist".
* The host counts every call per provider/event for Diagnostics, so new payload shapes
  are visible on real machines — turn them into fixtures.
* Implement `integration_status`, `install_integration`, `uninstall_integration` and
  `repair_integration` with: parse-or-refuse, timestamped backup, atomic write,
  ownership of our entries only (recognise them by program name + provider argument),
  idempotency and de-duplication. `crates/providers/ao-provider-claude/src/settings.rs`
  is the reference implementation.
* Preferences arrive in `configure(&ProviderSettings)`; background work (e.g. polling
  an official listing command) starts in `start(ctx)`.

## 6. Tests (required)

* Mapping tests: one fixture per provider event type → expected `AgentEvent`s, loaded
  with `ao_testkit::fixtures::load_dir("<provider>/…")` and checked with
  `assert_events` (subset matching; ids and timestamps are ignored).
* Protocol tests: a fake provider binary in `crates/ao-testkit/src/bin/` (see
  `fake-claude.rs`) that implements the documented CLI surface and runs hooks like
  the real CLI; build it from a test with `ao_testkit::bins::cargo_bin`. No real
  accounts or network.
* End-to-end: drive the real host with the fake binary and a temporary config folder
  (see `src-tauri/tests/claude_e2e.rs`).
* Capability test: the declared `CapabilityProfile` matches the doc table.
* Fault test: malformed input produces a `provider.error`, never a panic.
