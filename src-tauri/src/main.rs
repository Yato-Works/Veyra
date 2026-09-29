// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {:?}\n", info);
        eprintln!("{}", msg);
        let _ = std::fs::write("panic.log", msg);
    }));
    veyra_app_lib::run()
}
