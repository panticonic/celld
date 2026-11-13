// Turn on the decision core's observer hooks for this one compilation.
// Cargo reads build-script directives from stdout; nothing else prints here.
#![allow(clippy::disallowed_macros)]
fn main() {
    println!("cargo:rustc-cfg=celld_internal_tests");
    println!("cargo:rerun-if-changed=build.rs");
}
