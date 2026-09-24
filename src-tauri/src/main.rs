// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|a| a == "--smoke-test") {
        std::process::exit(agent_office_lib::smoke_test());
    }
    agent_office_lib::run();
}
