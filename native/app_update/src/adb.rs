//! Stops a local ADB server only when it uses this installation's bundled executable.

use anyhow::{Context, Result, ensure};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    time::Duration,
};

fn connect(command: &str) -> Result<TcpStream> {
    let address: SocketAddr = "127.0.0.1:5037".parse()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    write!(stream, "{:04x}{command}", command.len())?;
    let mut status = [0; 4];
    stream.read_exact(&mut status)?;
    ensure!(&status == b"OKAY", "ADB rejected {command}");
    Ok(stream)
}

pub fn stop_bundled_server(bundled: &Path) -> Result<()> {
    let mut stream = match connect("host:server-status") {
        Ok(stream) => stream,
        Err(_) => return Ok(()),
    };
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = usize::from_str_radix(std::str::from_utf8(&length)?, 16)?;
    ensure!(length <= 65535, "Invalid ADB server status length");
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let executable = executable_path(&bytes)?;
    if let Some(executable) = executable
        && Path::new(executable).canonicalize().ok() == Some(bundled.canonicalize()?)
    {
        connect("host:kill")?;
    }
    Ok(())
}

// AdbServerStatus field 7 is executable_absolute_path. Other fields are varints
// or length-delimited strings; unknown fields of those types are skipped.
fn executable_path(mut bytes: &[u8]) -> Result<Option<&str>> {
    let mut executable = None;
    while !bytes.is_empty() {
        let tag = varint(&mut bytes)?;
        match tag & 7 {
            0 => {
                varint(&mut bytes)?;
            }
            2 => {
                let length = usize::try_from(varint(&mut bytes)?)?;
                ensure!(length <= bytes.len(), "Truncated ADB status");
                let (value, rest) = bytes.split_at(length);
                bytes = rest;
                if tag >> 3 == 7 {
                    executable = Some(std::str::from_utf8(value)?);
                }
            }
            _ => anyhow::bail!("Unsupported ADB status field"),
        }
    }
    Ok(executable)
}

fn varint(bytes: &mut &[u8]) -> Result<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first().context("Truncated ADB status")?;
        *bytes = rest;
        ensure!(shift < 63 || byte <= 1, "Invalid ADB status integer");
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("Invalid ADB status integer")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_only_reported_server_executable() {
        assert_eq!(
            executable_path(b"\x08\x01\x3a\x08/opt/adb\x42\x01x").unwrap(),
            Some("/opt/adb")
        );
        assert_eq!(executable_path(b"\x08\x01").unwrap(), None);
        assert!(executable_path(b"\x3a\x08/opt").is_err());
    }
}
