#[cfg(feature = "binary")]
mod app;
#[cfg(feature = "binary")]
fn main() {
    app::main()
}

#[cfg(not(feature = "binary"))]
fn main() {
    println!("Use `cargo install --features binary` to install the binary version of the library.");
}
