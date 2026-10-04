//! `.svx` container: a single gzip member from offset 0, decompressed by
//! shelling out to the system `gzip` exactly as don-replay/don-net do (keeps
//! this crate dependency-free).

use std::io;
use std::path::Path;

#[derive(Debug)]
pub enum ContainerError {
    Io(io::Error),
    Gzip(String),
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::Io(e) => write!(f, "{e}"),
            ContainerError::Gzip(e) => write!(f, "gzip: {e}"),
        }
    }
}

impl std::error::Error for ContainerError {}

pub fn load_svx(path: &Path) -> Result<Vec<u8>, ContainerError> {
    let raw = std::fs::read(path).map_err(ContainerError::Io)?;
    if raw.len() < 2 {
        return Err(ContainerError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "short file")));
    }
    if raw[0] != 0x1F || raw[1] != 0x8B {
        return Ok(raw); // CTW map saves are stored raw
    }
    let out = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(path)
        .output()
        .map_err(ContainerError::Io)?;
    if !out.status.success() {
        return Err(ContainerError::Gzip(format!("exit {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr))));
    }
    Ok(out.stdout)
}
