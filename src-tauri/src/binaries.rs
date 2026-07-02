use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn find_binary(env_var: &str, binary_names: &[&str]) -> Option<PathBuf> {
    env::var_os(env_var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| is_executable_file(path))
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
