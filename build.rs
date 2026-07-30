fn main() {
    println!("cargo::rerun-if-changed=ansible");
    println!("cargo::rerun-if-changed=build.rs");
}
