//! What a build says of itself in telemetry (`telemetry::identity`): the target it was built for, and the channel and
//! commit the release scripts of `quickai_base` set (`QUICKAI_CHANNEL=official`, `QUICKAI_COMMIT`).
fn main() {
    println!(
        "cargo:rustc-env=QUICKAI_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    println!("cargo:rerun-if-env-changed=QUICKAI_CHANNEL");
    println!("cargo:rerun-if-env-changed=QUICKAI_COMMIT");
    println!("cargo:rerun-if-changed=build.rs");
}
