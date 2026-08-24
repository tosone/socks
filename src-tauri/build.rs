fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/packet_tunnel.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("packet_tunnel");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=NetworkExtension");
        println!("cargo:rerun-if-changed=native/packet_tunnel.m");
    }
    tauri_build::build()
}
