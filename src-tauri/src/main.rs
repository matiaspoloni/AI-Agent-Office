// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `agent-office hook <provider> …` is the hook relay invoked by agent CLIs.
    // It must stay tiny and fast: dispatch before anything else is initialized.
    if std::env::args().nth(1).as_deref() == Some("hook") {
        std::process::exit(ao_hook_relay::main_from_env());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("integrations") {
        std::process::exit(agent_office_lib::integrations_command(&args[1..]));
    }
    if std::env::args().any(|a| a == "--smoke-test") {
        std::process::exit(agent_office_lib::smoke_test());
    }
    agent_office_lib::run();
}
