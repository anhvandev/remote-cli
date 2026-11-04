fn main() {
    // protoc is vendored so no system install is needed.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    println!("cargo:rerun-if-changed=proto");
    prost_build::compile_protos(&["proto/agent.proto"], &["proto"]).unwrap();
}
