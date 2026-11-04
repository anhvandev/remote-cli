fn main() {
    // protoc is vendored so no system install is needed.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    println!("cargo:rerun-if-changed=proto");
    let mut config = prost_build::Config::new();
    // Stable JSON output for maps.
    config.btree_map(["."]);
    // The Controller API returns these as JSON as-is.
    for t in ["Inventory", "Disk", "NetworkInterface", "Display", "Resources", "Process"] {
        config.type_attribute(format!(".rcli.agent.v1.{t}"), "#[derive(serde::Serialize)]");
    }
    config
        .field_attribute(".rcli.agent.v1.Resources.session", "#[serde(serialize_with = \"crate::serialize_session\")]");
    config.compile_protos(&["proto/agent.proto"], &["proto"]).unwrap();
}
