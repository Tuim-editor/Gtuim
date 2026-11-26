use std::sync::atomic::Ordering;

use gtuim::gui;
use gtuim::metrics::GLOBAL;

fn print_help() {
    println!(
        "Gtuim {} — Native GPUI Workspace & Modal/Modeless IDE Editor (powered by Neovim)\n\n\
         USAGE:\n  \
         gtuim [OPTIONS] [FILES...]\n\n\
         OPTIONS:\n  \
         -h, --help          Print this help information\n  \
         -V, --version       Print version information\n  \
         --diagnostics       Enable runtime performance diagnostics",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files_to_open: Vec<String> = Vec::new();
    let mut diagnostics_enabled = false;
    let mut passthrough = false;

    for arg in &args {
        if passthrough {
            files_to_open.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--" => passthrough = true,
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-V" | "--version" => {
                println!("gtuim {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--diagnostics" => {
                diagnostics_enabled = true;
            }
            s if s.starts_with('-') => {
                eprintln!("Gtuim could not start: unknown option '{}'\n", s);
                print_help();
                std::process::exit(2);
            }
            _ => {
                files_to_open.push(arg.clone());
            }
        }
    }

    if diagnostics_enabled || std::env::var("TUIM_DIAGNOSTICS").ok().as_deref() == Some("1") {
        GLOBAL.enabled.store(true, Ordering::Relaxed);
    }

    gui::run_gui(files_to_open);
}
