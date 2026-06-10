fn main() {
    // The screencapturekit crate includes a Swift bridge, so make the Swift
    // runtime (libswift_Concurrency etc.) resolvable via rpath
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build()
}
