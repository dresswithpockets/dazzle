use copy_to_output::copy_to_output;
use std::env;

fn main() {
    println!("cargo:rerun-if-changed=backup/*");
    copy_to_output("backup", &env::var("PROFILE").unwrap()).expect("Could not copy");
}
