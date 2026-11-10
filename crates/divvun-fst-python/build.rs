// An extension module leaves its Python symbols for the interpreter that loads
// it. maturin tells Apple's linker to look them up at load time; this does the
// same for a plain `cargo build`, which otherwise refuses them. iOS and
// visionOS are Darwin too.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rustc-cdylib-link-arg=-undefined");
        println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
    }
}
