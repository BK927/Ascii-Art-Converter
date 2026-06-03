fn main() {
    #[cfg(windows)]
    {
        use std::env;
        use std::fs;
        use std::path::PathBuf;
        use std::process::Command;

        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let icon_path = manifest_dir
            .join("../../assets/icons/aa-converter-icon.ico")
            .canonicalize()
            .expect("app icon should exist");
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let resource_script = out_dir.join("aa-egui.rc");
        let resource_file = out_dir.join("aa-egui.res");
        let escaped_icon_path = icon_path.to_string_lossy().replace('\\', "\\\\");
        let rc_exe = find_resource_compiler();

        fs::write(
            &resource_script,
            format!("1 ICON \"{escaped_icon_path}\"\n"),
        )
        .expect("failed to write Windows resource script");

        let status = Command::new(&rc_exe)
            .arg("/nologo")
            .arg(format!("/fo{}", resource_file.display()))
            .arg(&resource_script)
            .status()
            .unwrap_or_else(|err| panic!("failed to run {}: {err}", rc_exe.display()));

        if !status.success() {
            panic!("rc.exe failed to compile Windows resources");
        }

        println!("cargo:rerun-if-changed={}", icon_path.display());
        println!(
            "cargo:rustc-link-arg-bin=aa-egui={}",
            resource_file.display()
        );
    }
}

#[cfg(windows)]
fn find_resource_compiler() -> std::path::PathBuf {
    use std::env;
    use std::path::PathBuf;

    if let Some(path) = env::var_os("RC").map(PathBuf::from) {
        return path;
    }

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let sdk_arch = match target_arch.as_str() {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };

    let mut roots = Vec::new();
    if let Some(path) = env::var_os("WindowsSdkDir").map(PathBuf::from) {
        roots.push(path.join("bin"));
    }
    if let Some(path) = env::var_os("ProgramFiles(x86)").map(PathBuf::from) {
        roots.push(path.join("Windows Kits/10/bin"));
    }
    if let Some(path) = env::var_os("ProgramFiles").map(PathBuf::from) {
        roots.push(path.join("Windows Kits/10/bin"));
    }

    for root in roots {
        if let Some(rc_exe) = find_windows_sdk_rc(&root, sdk_arch) {
            return rc_exe;
        }
    }

    PathBuf::from("rc.exe")
}

#[cfg(windows)]
fn find_windows_sdk_rc(root: &std::path::Path, sdk_arch: &str) -> Option<std::path::PathBuf> {
    let legacy_path = root.join(sdk_arch).join("rc.exe");
    if legacy_path.exists() {
        return Some(legacy_path);
    }

    let mut versioned_dirs = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| looks_like_sdk_version(name))
        })
        .collect::<Vec<_>>();
    versioned_dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    versioned_dirs
        .into_iter()
        .map(|path| path.join(sdk_arch).join("rc.exe"))
        .find(|path| path.exists())
}

#[cfg(windows)]
fn looks_like_sdk_version(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy()
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.')
}
