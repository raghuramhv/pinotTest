fn main() {
    // Placeholder for future protobuf compilation if needed
    // tonic_build::compile_protos("proto/mailbox.proto").unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
