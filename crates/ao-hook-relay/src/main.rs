//! Standalone relay binary (development and tests). The shipped app runs the
//! same code as `agent-office hook …`.

fn main() {
    std::process::exit(ao_hook_relay::main_from_env());
}
