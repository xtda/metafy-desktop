use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use zip::ZipArchive;

const BUNDLED_RESOURCE_DIRECTORY: &str = "binaries";
const BUNDLED_TOOLS_DIRECTORY: &str = "tools";
const BUNDLED_TOOLS_VERSION: &str = "v1";
const INSTALL_MANIFEST_FILE: &str = ".metafy-bundled-tools";
const KNOWN_BUNDLED_BINARY_NAMES: &[&str] =
    &["ffmpeg", "ffprobe", "whisper-cli", "main", "whisper"];

static BUNDLED_TOOLS_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn initialize_bundled_binaries(app_handle: &AppHandle) -> Result<Option<PathBuf>, String> {
    let install_root = app_handle
        .path()
        .app_data_dir()
        .map_err(|error| format!("Unable to resolve app data directory: {error}"))?
        .join(BUNDLED_TOOLS_DIRECTORY)
        .join(BUNDLED_TOOLS_VERSION)
        .join(platform_resource_directory());
    let _ = BUNDLED_TOOLS_ROOT.set(install_root.clone());

    let Some(archive_directory) = bundled_archive_directory(app_handle) else {
        return Ok(None);
    };
    if !archive_directory.is_dir() {
        return Ok(None);
    }

    let archives = bundled_archives(&archive_directory)?;
    if archives.is_empty() {
        return Ok(None);
    }

    let fingerprint = archive_fingerprint(&archives)?;
    if installation_matches(&install_root, &fingerprint) {
        return Ok(Some(install_root));
    }

    extract_bundled_archives(&archives, &install_root, &fingerprint)?;
    Ok(Some(install_root))
}

pub fn find_binary(env_var: &str, binary_names: &[&str]) -> Option<PathBuf> {
    env::var_os(env_var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| is_executable_file(path))
        .or_else(|| find_binary_in_bundled_tools(binary_names))
        .or_else(|| find_binary_in_path(binary_names))
        .or_else(|| find_binary_in_common_locations(binary_names))
}

pub fn missing_binary_message(binary_name: &str, env_var: &str) -> String {
    format!(
        "{binary_name} is not available. Install it on PATH or set {env_var} to the binary path."
    )
}

fn find_binary_in_path(binary_names: &[&str]) -> Option<PathBuf> {
    let path_value = env::var_os("PATH")?;
    find_binary_in_directories(env::split_paths(&path_value), binary_names)
}

fn find_binary_in_bundled_tools(binary_names: &[&str]) -> Option<PathBuf> {
    let root = BUNDLED_TOOLS_ROOT.get()?;
    find_binary_in_directories(bundled_binary_directories(root), binary_names)
}

fn find_binary_in_common_locations(binary_names: &[&str]) -> Option<PathBuf> {
    find_binary_in_directories(
        common_binary_directories().iter().map(PathBuf::from),
        binary_names,
    )
}

fn find_binary_in_directories<I>(directories: I, binary_names: &[&str]) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    directories.into_iter().find_map(|directory| {
        binary_names.iter().find_map(|binary_name| {
            candidate_names(binary_name)
                .into_iter()
                .map(|name| directory.join(name))
                .find(|path| is_executable_file(path))
        })
    })
}

fn common_binary_directories() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
    }

    #[cfg(target_os = "linux")]
    {
        &[
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/snap/bin",
            "/app/bin",
        ]
    }

    #[cfg(windows)]
    {
        &[]
    }
}

fn candidate_names(binary_name: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let path_ext = env::var_os("PATHEXT")
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| ".EXE;.BAT;.CMD".to_owned());
        let mut names = vec![OsString::from(binary_name)];
        names.extend(
            path_ext
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| {
                    let mut name = OsString::from(binary_name);
                    name.push(ext);
                    name
                }),
        );
        names
    }

    #[cfg(not(windows))]
    {
        vec![OsString::from(binary_name)]
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn bundled_archive_directory(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle.path().resource_dir().ok().map(|resource_dir| {
        resource_dir
            .join(BUNDLED_RESOURCE_DIRECTORY)
            .join(platform_resource_directory())
    })
}

fn bundled_archives(archive_directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut archives = Vec::new();
    for entry in fs::read_dir(archive_directory)
        .map_err(|error| format!("Unable to inspect bundled binary resources: {error}"))?
    {
        let path = entry
            .map_err(|error| format!("Unable to inspect bundled binary resource: {error}"))?
            .path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            archives.push(path);
        }
    }
    archives.sort();
    Ok(archives)
}

fn archive_fingerprint(archives: &[PathBuf]) -> Result<String, String> {
    let mut fingerprint = format!(
        "version={BUNDLED_TOOLS_VERSION}\nplatform={}\n",
        platform_resource_directory()
    );

    for archive in archives {
        let metadata = fs::metadata(archive)
            .map_err(|error| format!("Unable to inspect bundled archive: {error}"))?;
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let archive_name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        fingerprint.push_str(&format!(
            "{archive_name}:{}:{modified_at}\n",
            metadata.len()
        ));
    }

    Ok(fingerprint)
}

fn installation_matches(install_root: &Path, fingerprint: &str) -> bool {
    fs::read_to_string(install_root.join(INSTALL_MANIFEST_FILE))
        .map(|existing| existing == fingerprint)
        .unwrap_or(false)
}

fn extract_bundled_archives(
    archives: &[PathBuf],
    install_root: &Path,
    fingerprint: &str,
) -> Result<(), String> {
    let parent = install_root
        .parent()
        .ok_or_else(|| "Unable to resolve bundled tool install parent.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Unable to prepare bundled tool directory: {error}"))?;

    let staging_root = parent.join(format!(
        ".{}-{}-{}",
        install_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tools"),
        process::id(),
        now_millis()
    ));
    remove_directory_if_exists(&staging_root)?;
    fs::create_dir_all(&staging_root)
        .map_err(|error| format!("Unable to prepare bundled tool staging directory: {error}"))?;

    if let Err(error) = prepare_staged_bundled_tools(archives, &staging_root, fingerprint) {
        let _ = fs::remove_dir_all(&staging_root);
        return Err(error);
    }

    replace_directory(&staging_root, install_root)
}

fn prepare_staged_bundled_tools(
    archives: &[PathBuf],
    staging_root: &Path,
    fingerprint: &str,
) -> Result<(), String> {
    for archive in archives {
        extract_zip_archive(archive, staging_root)?;
    }

    ensure_known_binaries_are_executable(staging_root)?;

    let mut manifest = File::create(staging_root.join(INSTALL_MANIFEST_FILE))
        .map_err(|error| format!("Unable to write bundled tool manifest: {error}"))?;
    manifest
        .write_all(fingerprint.as_bytes())
        .map_err(|error| format!("Unable to write bundled tool manifest: {error}"))
}

fn extract_zip_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let archive_file = File::open(archive_path).map_err(|error| {
        format!(
            "Unable to open bundled archive {}: {error}",
            archive_path.display()
        )
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|error| {
        format!(
            "Unable to read bundled archive {}: {error}",
            archive_path.display()
        )
    })?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            format!(
                "Unable to read bundled archive entry from {}: {error}",
                archive_path.display()
            )
        })?;
        let enclosed_name = entry.enclosed_name().ok_or_else(|| {
            format!(
                "Bundled archive {} contains an unsafe path: {}",
                archive_path.display(),
                entry.name()
            )
        })?;
        reject_symlink_entry(archive_path, entry.name(), entry.unix_mode())?;

        let output_path = destination.join(enclosed_name);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| {
                format!(
                    "Unable to create bundled tool directory {}: {error}",
                    output_path.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Unable to create bundled tool directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        let mut output_file = File::create(&output_path).map_err(|error| {
            format!(
                "Unable to create bundled tool file {}: {error}",
                output_path.display()
            )
        })?;
        io::copy(&mut entry, &mut output_file).map_err(|error| {
            format!(
                "Unable to extract bundled tool file {}: {error}",
                output_path.display()
            )
        })?;
        set_file_mode_from_zip(&output_path, entry.unix_mode())?;
    }

    Ok(())
}

fn replace_directory(staging_root: &Path, install_root: &Path) -> Result<(), String> {
    if install_root.exists() {
        let backup_root = install_root.with_file_name(format!(
            ".{}-backup-{}-{}",
            install_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("tools"),
            process::id(),
            now_millis()
        ));
        remove_directory_if_exists(&backup_root)?;
        fs::rename(install_root, &backup_root)
            .map_err(|error| format!("Unable to replace bundled tools: {error}"))?;
        if let Err(error) = fs::rename(staging_root, install_root) {
            let _ = fs::rename(&backup_root, install_root);
            return Err(format!("Unable to activate bundled tools: {error}"));
        }
        let _ = fs::remove_dir_all(&backup_root);
        Ok(())
    } else {
        fs::rename(staging_root, install_root)
            .map_err(|error| format!("Unable to activate bundled tools: {error}"))
    }
}

fn remove_directory_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Unable to remove {}: {error}", path.display())),
    }
}

fn bundled_binary_directories(root: &Path) -> Vec<PathBuf> {
    let mut directories = vec![root.to_path_buf(), root.join("bin")];
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path.clone());
                directories.push(path.join("bin"));
            }
        }
    }
    directories
}

fn platform_resource_directory() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(unix)]
fn reject_symlink_entry(
    archive_path: &Path,
    entry_name: &str,
    mode: Option<u32>,
) -> Result<(), String> {
    const FILE_TYPE_MASK: u32 = 0o170000;
    const SYMLINK_TYPE: u32 = 0o120000;
    if mode.is_some_and(|mode| mode & FILE_TYPE_MASK == SYMLINK_TYPE) {
        return Err(format!(
            "Bundled archive {} contains unsupported symlink entry: {entry_name}",
            archive_path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_symlink_entry(
    _archive_path: &Path,
    _entry_name: &str,
    _mode: Option<u32>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_from_zip(path: &Path, mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let Some(mode) = mode else {
        return Ok(());
    };
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
        .map_err(|error| format!("Unable to set bundled tool permissions: {error}"))
}

#[cfg(not(unix))]
fn set_file_mode_from_zip(_path: &Path, _mode: Option<u32>) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn ensure_known_binaries_are_executable(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    for directory in bundled_binary_directories(root) {
        for binary_name in KNOWN_BUNDLED_BINARY_NAMES {
            let path = directory.join(binary_name);
            if !path.is_file() {
                continue;
            }

            let metadata = fs::metadata(&path).map_err(|error| {
                format!("Unable to inspect bundled tool {}: {error}", path.display())
            })?;
            let mut permissions = metadata.permissions();
            let mode = permissions.mode();
            if mode & 0o111 == 0 {
                permissions.set_mode(mode | 0o755);
                fs::set_permissions(&path, permissions).map_err(|error| {
                    format!(
                        "Unable to make bundled tool executable {}: {error}",
                        path.display()
                    )
                })?;
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_known_binaries_are_executable(_root: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use uuid::Uuid;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    #[test]
    fn extracts_zip_archive_and_marks_known_binaries_executable() {
        let root = test_directory("extracts-zip");
        fs::create_dir_all(&root).expect("create test root");
        let archive_path = root.join("tools.zip");
        let destination = root.join("out");

        create_test_zip(&archive_path, "bin/ffmpeg", b"fake ffmpeg", 0o644);

        extract_zip_archive(&archive_path, &destination).expect("extract archive");
        ensure_known_binaries_are_executable(&destination).expect("set executable bits");

        let extracted = destination.join("bin").join("ffmpeg");
        assert_eq!(
            fs::read_to_string(&extracted).expect("read extracted binary"),
            "fake ffmpeg"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = fs::metadata(&extracted)
                .expect("read extracted metadata")
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_zip_entries_that_escape_destination() {
        let root = test_directory("rejects-unsafe-zip");
        fs::create_dir_all(&root).expect("create test root");
        let archive_path = root.join("tools.zip");
        let destination = root.join("out");

        create_test_zip(&archive_path, "../ffmpeg", b"fake ffmpeg", 0o755);

        let error = extract_zip_archive(&archive_path, &destination)
            .expect_err("unsafe archive should fail");
        assert!(error.contains("unsafe path"));
        assert!(!root.join("ffmpeg").exists());

        let _ = fs::remove_dir_all(root);
    }

    fn create_test_zip(path: &Path, entry_name: &str, content: &[u8], mode: u32) {
        let file = File::create(path).expect("create test archive");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(mode);
        zip.start_file(entry_name, options)
            .expect("start zip entry");
        zip.write_all(content).expect("write zip entry");
        zip.finish().expect("finish zip");
    }

    fn test_directory(name: &str) -> PathBuf {
        env::temp_dir().join(format!("metafy-{name}-{}", Uuid::new_v4()))
    }
}
