use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use stlmunge;

fn main() {
    linker_script_plumbing();
    build_assembly_sources();
    munge_rook_stl();
}

fn build_assembly_sources() {
    cc::Build::new()
        .file("src/asm/unpack_1bpp.S")
        .file("src/asm/unpack_1bpp_overlay.S")
        .file("src/asm/unpack_text_10p_attributed.S")
        .file("src/asm/copy_words.S")
        .compile("libunrusted.a");
    println!("cargo:rerun-if-changed=src/asm/copy_words.S");
    println!("cargo:rerun-if-changed=src/asm/unpack_1bpp.S");
    println!("cargo:rerun-if-changed=src/asm/unpack_1bpp_overlay.S");
    println!("cargo:rerun-if-changed=src/asm/unpack_text_10p_attributed.S");

    cc::Build::new()
        .file("src/bin/xor_pattern/pattern.S")
        .compile("libxor_pattern.a");
    println!("cargo:rerun-if-changed=src/bin/xor_pattern/pattern.S");

    cc::Build::new()
        .file("src/bin/poly3/fill.S")
        .compile("libpoly3_fill.a");
    println!("cargo:rerun-if-changed=src/bin/poly3/fill.S");
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

fn munge_rook_stl() {
    let input = File::open("src/bin/rook/model.stl").unwrap();

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap())
        .join("rook_model_include.rs");
    let output = File::create(out).unwrap();

    stlmunge::generate(input, output).unwrap();

    println!("cargo:rerun-if-changed=src/bin/rook/model.stl");
}
