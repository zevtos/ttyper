use std::{
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, Once, OnceLock},
    time::Duration,
};

const BORE_RELEASE_API: &str = "https://api.github.com/repos/ekzhang/bore/releases/latest";
const BORE_INSTALL_ERROR: &str =
    "Could not install bore automatically. Please run: cargo install bore-cli";

type SharedChild = Arc<Mutex<Option<Child>>>;

static REGISTERED_CHILD: OnceLock<Mutex<Option<SharedChild>>> = OnceLock::new();
static CLEANUP_HOOKS: Once = Once::new();

pub struct BoreTunnel {
    public_addr: String,
    child: SharedChild,
}

impl BoreTunnel {
    pub fn public_addr(&self) -> &str {
        &self.public_addr
    }
}

impl Drop for BoreTunnel {
    fn drop(&mut self) {
        kill_child(&self.child);
    }
}

pub fn register_cleanup_handlers() {
    CLEANUP_HOOKS.call_once(|| {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            kill_registered_tunnel();
            previous_hook(panic_info);
        }));

        let _ = ctrlc::set_handler(|| {
            kill_registered_tunnel();
        });
    });
}

pub fn ensure_bore_installed() -> io::Result<PathBuf> {
    if let Some(path) = find_bore() {
        return Ok(path);
    }

    println!("bore not found. Installing bore-cli automatically, please wait...");
    let installed = install_bore()?;
    if command_succeeds(&installed, "--version") {
        return Ok(installed);
    }
    if let Some(path) = find_bore() {
        return Ok(path);
    }

    Err(io::Error::new(io::ErrorKind::NotFound, BORE_INSTALL_ERROR))
}

pub fn spawn_bore(bore_path: &Path, local_port: u16, server: &str) -> io::Result<BoreTunnel> {
    let mut child = Command::new(bore_path)
        .args(["local", &local_port.to_string(), "--to", server])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| io::Error::new(error.kind(), format!("failed to start bore: {error}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "could not read bore stdout"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    let public_addr = loop {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                if let Some(addr) = parse_public_addr(&line) {
                    break addr;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait()? {
                    return Err(io::Error::other(format!(
                        "bore exited before creating tunnel: {status}"
                    )));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "bore closed stdout before creating tunnel",
                ));
            }
        }
    };

    let child = Arc::new(Mutex::new(Some(child)));
    register_child(child.clone());
    Ok(BoreTunnel { public_addr, child })
}

pub fn kill_registered_tunnel() {
    let Some(registry) = REGISTERED_CHILD.get() else {
        return;
    };
    let child = registry.lock().ok().and_then(|mut child| child.take());
    if let Some(child) = child {
        kill_child(&child);
    }
}

fn register_child(child: SharedChild) {
    let registry = REGISTERED_CHILD.get_or_init(|| Mutex::new(None));
    if let Ok(mut registered) = registry.lock() {
        *registered = Some(child);
    }
}

fn kill_child(child: &SharedChild) {
    let Ok(mut child) = child.lock() else {
        return;
    };
    if let Some(mut child) = child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn find_bore() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(executable_name("bore")),
        current_exe_dir().join(executable_name("bore")),
        cargo_bin_dir().join(executable_name("bore")),
    ];

    candidates
        .into_iter()
        .find(|candidate| command_succeeds(candidate, "--version"))
}

fn command_succeeds(path: &Path, arg: &str) -> bool {
    Command::new(path)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
fn install_bore() -> io::Result<PathBuf> {
    let target = cargo_bin_dir().join("bore.exe");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$target = $env:TTYPER_BORE_TARGET
$release = Invoke-RestMethod -Headers @{{ 'User-Agent' = 'ttyper' }} -Uri '{BORE_RELEASE_API}'
$asset = $release.assets | Where-Object {{ $_.name -like '*x86_64-pc-windows-msvc.zip' }} | Select-Object -First 1
if (-not $asset) {{ throw 'Could not find Windows bore release asset' }}
$zip = Join-Path $env:TEMP ('bore-' + [guid]::NewGuid().ToString() + '.zip')
$dir = Join-Path $env:TEMP ('bore-' + [guid]::NewGuid().ToString())
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $dir -Force
$binary = Get-ChildItem -Path $dir -Recurse -Filter bore.exe | Select-Object -First 1
if (-not $binary) {{ throw 'Downloaded bore archive did not contain bore.exe' }}
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
Copy-Item $binary.FullName $target -Force
Remove-Item $zip -Force
Remove-Item $dir -Recurse -Force
"#
    );

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .env("TTYPER_BORE_TARGET", &target)
        .status()?;
    if status.success() {
        Ok(target)
    } else {
        Err(io::Error::other(BORE_INSTALL_ERROR))
    }
}

#[cfg(not(windows))]
fn install_bore() -> io::Result<PathBuf> {
    let status = Command::new("cargo")
        .args(["install", "bore-cli"])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(BORE_INSTALL_ERROR));
    }

    find_bore().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, BORE_INSTALL_ERROR))
}

fn parse_public_addr(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|part| part.starts_with("bore.pub:"))
        .map(|part| part.trim_matches(|character| character == '.' || character == ','))
        .map(ToOwned::to_owned)
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn current_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cargo_bin_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cargo")
        .join("bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bore_public_address() {
        assert_eq!(
            parse_public_addr("listening at bore.pub:43821"),
            Some("bore.pub:43821".into())
        );
        assert_eq!(parse_public_addr("not ready"), None);
    }
}
