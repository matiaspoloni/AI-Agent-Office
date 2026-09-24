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
        }
    }

    fn capabilities(&self) -> CapabilityProfile { /* honest values from the doc */ }

    async fn detect_installation(&self) -> Result<InstallationInfo, ProviderError> {
        ao_detect::detect(&self.descriptor().executable_names, &["--version"]).await
    }
    // Implement only what the provider really supports.
    // Everything else keeps the default: Err(ProviderError::Unsupported { .. }).
}
```

Rules:

* **Push, don't pull.** Emit normalized events through the `EventSink` in
  `AdapterContext`. Never talk to the UI directly.
* **Map in pure functions** (`mapping.rs`) so they can be tested with fixtures.
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

Reuse `agent-office-hook.exe`: configure the provider's hook to run
`agent-office-hook.exe <provider-id>` in exec form. Implement
`handle_hook()` to map the payload and, for decision events, return the
provider-specific stdout. Implement `install_integration`, `uninstall_integration`,
`repair_integration`, `integration_status` with: parse-or-refuse, backup, atomic
write, marker-based ownership of our entries, idempotency.

## 6. Tests (required)

* Mapping tests: one fixture per provider event type → expected `AgentEvent`s.
* Protocol tests: a fake provider binary (see `fixtures/` and the test helpers) that
  replays recorded traffic; no real accounts or network.
* Capability test: the declared `CapabilityProfile` matches the doc table.
* Fault test: malformed input produces a `provider.error`, never a panic.
