fn main() {
    for path in [
        "../uart_driver/src",
        "../uart_driver/Cargo.toml",
        "../uart_driver/Cargo.lock",
        "../uart_driver/build.sh",
        "../tty_driver/src",
        "../tty_driver/Cargo.toml",
        "../tty_driver/Cargo.lock",
        "../tty_driver/build.sh",
        "../serverlib/src",
        "../serverlib/Cargo.toml",
        "../serverlib/linker/server.ld",
        "rvem_layout.ld",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=UART_HEAP_SIZE");
    println!("cargo:rerun-if-env-changed=TTY_HEAP_SIZE");
    for server in ["uart_driver", "tty_driver"] {
        let status = std::process::Command::new("sh")
            .arg(format!("../{server}/build.sh"))
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("failed to run server build");
        assert!(status.success(), "{server} build failed");
    }
}
