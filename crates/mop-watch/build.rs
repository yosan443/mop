use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let target_lib_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .map(|p| p.join("lib"));

    if let Some(target_lib) = target_lib_dir {
        let _ = std::fs::create_dir_all(&target_lib);
        let link_target = target_lib.join("libsystemd.so");

        let candidate_libs = [
            "/lib/x86_64-linux-gnu/libsystemd.so.0",
            "/usr/lib/x86_64-linux-gnu/libsystemd.so.0",
            "/lib/aarch64-linux-gnu/libsystemd.so.0",
            "/usr/lib/aarch64-linux-gnu/libsystemd.so.0",
            "/lib/libsystemd.so.0",
            "/usr/lib/libsystemd.so.0",
        ];

        for cand in &candidate_libs {
            if Path::new(cand).exists() && !link_target.exists() {
                #[cfg(unix)]
                let _ = std::os::unix::fs::symlink(cand, &link_target);
                break;
            }
        }

        println!("cargo:rustc-link-search=native={}", target_lib.display());
    }

    println!("cargo:rustc-link-search=native=/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-search=native=/lib/aarch64-linux-gnu");
    println!("cargo:rustc-link-search=native=/usr/lib/aarch64-linux-gnu");
}
