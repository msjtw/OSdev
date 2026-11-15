fn main() {
    for path in [
        "../uart_driver/src",
        "../uart_driver/Cargo.toml",
        "../uart_driver/Cargo.lock",
        "../uart_driver/build.sh",
        "../serverlib/src",
        "../serverlib/Cargo.toml",
        "../serverlib/linker/server.ld",
        "rvem_layout.ld",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=UART_HEAP_SIZE");
    let status = std::process::Command::new("sh")
        .arg("../uart_driver/build.sh")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("failed to run UART build");
    assert!(status.success(), "UART build failed");
}
