use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use stlmunge;

fn main() {
    build_assembly_sources();

    munge_rook_stl();
    munge_solid_stl();
}

fn build_assembly_sources() {
    cc::Build::new()
        .file("src/bin/xor_pattern/pattern.S")
        .compile("libxor_pattern.a");
    println!("cargo:rerun-if-changed=src/bin/xor_pattern/pattern.S");

    cc::Build::new()
        .file("src/bin/poly3/fill.S")
        .compile("libpoly3_fill.a");
    println!("cargo:rerun-if-changed=src/bin/poly3/fill.S");
}

fn munge_rook_stl() {
    let input = File::open("src/bin/rook/model.stl").unwrap();

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap())
        .join("rook_model_include.rs");
    let output = File::create(out).unwrap();

    stlmunge::generate_wireframe(input, output).unwrap();

    println!("cargo:rerun-if-changed=src/bin/rook/model.stl");
}

fn munge_solid_stl() {
    let input = File::open("src/bin/poly3/model.stl").unwrap();

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap())
        .join("poly3_model_include.rs");
    let output = File::create(out).unwrap();

    stlmunge::generate_solid(input, output).unwrap();

    println!("cargo:rerun-if-changed=src/bin/poly3/model.stl");
}
