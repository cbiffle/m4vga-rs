use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let os_target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let simulation = os_target != "none";

    if !simulation {
        linker_script_plumbing();
        build_assembly_sources();
    }
}

fn build_assembly_sources() {
    cc::Build::new()
        .file("src/asm/unpack_1bpp.S")
        .file("src/asm/unpack_text_10p_attributed.S")
        .file("src/asm/copy_words.S")
        .compile("libunrusted.a");
    println!("cargo:rerun-if-changed=src/asm/copy_words.S");
    println!("cargo:rerun-if-changed=src/asm/unpack_1bpp.S");
    println!("cargo:rerun-if-changed=src/asm/unpack_text_10p_attributed.S");
}

fn linker_script_plumbing() {
    // Put the linker script somewhere the linker can find it
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=link-custom.x");
}
