//! Unpack a Salvaê backup (`.svpk`) into a folder, for manual save recovery.
//! Usage: cargo run -p salvae-sync --example unpack -- <file.svpk> <dest_dir>

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: unpack <file.svpk> <dest_dir>");
        std::process::exit(2);
    }
    let packed = std::fs::read(&args[1]).expect("read .svpk");
    salvae_sync::pack::unpack_folder(&packed, Path::new(&args[2])).expect("unpack");
    println!("Unpacked {} into {}", args[1], args[2]);
}
