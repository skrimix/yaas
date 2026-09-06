//! ADB operations used to start, recover, and stop casting.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use forensic_adb::{Device, DeviceState, ShellOutput};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

use crate::session::Result;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const DEVICE_POLL_INTERVAL: Duration = Duration::from_millis(100);

async fn bounded<T>(operation: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(COMMAND_TIMEOUT, operation)
        .await
        .map_err(|_| "ADB command timed out")?
}

fn shell_command(args: &[&str]) -> String {
    args.iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

async fn shell_output(device: &Device, args: &[&str]) -> Result<ShellOutput> {
    let command = shell_command(args);
    let output = bounded(async { Ok(device.shell_v2(&command).await?) }).await?;
    debug!(
        command,
        exit_code = output.exit_code,
        stdout = %String::from_utf8_lossy(&output.stdout).trim(),
        stderr = %String::from_utf8_lossy(&output.stderr).trim(),
        "ADB shell command finished"
    );
    Ok(output)
}

pub(crate) async fn shell(device: &Device, args: &[&str]) -> Result<String> {
    let output = shell_output(device, args).await?;
    if output.exit_code != 0 {
        return Err(format!(
            "ADB shell command exited with {}: {args:?}: {}",
            output.exit_code,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(crate) async fn shell_best_effort(device: &Device, args: &[&str]) -> Result<()> {
    shell_output(device, args).await?;
    Ok(())
}

pub(crate) async fn reverse(device: &Device, port: u16) -> Result<()> {
    bounded(async {
        device.reverse_port(port, port).await?;
        Ok(())
    })
    .await
}

pub(crate) async fn remove_reverse(device: &Device, port: u16) -> Result<()> {
    // The library expects a length-prefixed reply here, but adbd sends only a status.
    bounded(async {
        let mut stream = connect(device).await?;
        request(&mut stream, &format!("host:transport:{}", device.serial)).await?;
        request(&mut stream, &format!("reverse:killforward:tcp:{port}")).await?;
        read_status(&mut stream).await
    })
    .await
}

pub(crate) async fn device_available(device: &Device) -> Result<bool> {
    bounded(async {
        let devices = device.host.devices::<Vec<_>>().await?;
        Ok(devices
            .iter()
            .any(|entry| entry.serial == device.serial && entry.state == DeviceState::Device))
    })
    .await
}

pub(crate) async fn wait_for_device(device: &Device, stop: &AtomicBool) -> Result<bool> {
    let wait = async {
        loop {
            if device_available(device).await? {
                return Ok(true);
            }
            tokio::time::sleep(DEVICE_POLL_INTERVAL).await;
        }
    };
    let cancelled = async {
        while !stop.load(Ordering::SeqCst) {
            tokio::time::sleep(DEVICE_POLL_INTERVAL).await;
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Ok(false),
        result = wait => result,
    }
}

// forensic-adb has no reconnect method. Send the serial-scoped service to the same ADB server.
pub(crate) async fn reconnect(device: &Device) -> Result<()> {
    bounded(async {
        let mut stream = connect(device).await?;
        request(
            &mut stream,
            &format!("host-serial:{}:reconnect", device.serial),
        )
        .await?;
        let message = read_message(&mut stream).await?;
        debug!(%message, "ADB reconnect requested");
        Ok(())
    })
    .await
}

async fn connect(device: &Device) -> Result<TcpStream> {
    Ok(TcpStream::connect((
        device.host.host.as_deref().unwrap_or("localhost"),
        device.host.port.unwrap_or(5037),
    ))
    .await?)
}

async fn request(stream: &mut TcpStream, command: &str) -> Result<()> {
    let length = u16::try_from(command.len())?;
    stream
        .write_all(format!("{length:04X}{command}").as_bytes())
        .await?;
    read_status(stream).await
}

async fn read_status(stream: &mut TcpStream) -> Result<()> {
    let mut status = [0; 4];
    stream.read_exact(&mut status).await?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => Err(format!("ADB request failed: {}", read_message(stream).await?).into()),
        _ => Err(format!("Invalid ADB status: {status:?}").into()),
    }
}

async fn read_message(stream: &mut TcpStream) -> Result<String> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).await?;
    let length = usize::from_str_radix(std::str::from_utf8(&length)?, 16)?;
    let mut message = vec![0; length];
    stream.read_exact(&mut message).await?;
    Ok(String::from_utf8_lossy(&message).into_owned())
}

#[cfg(test)]
pub(crate) fn test_device() -> Device {
    Device {
        host: forensic_adb::Host::default(),
        serial: "test-device".to_string(),
        info: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    type Exchange = (String, Vec<u8>);

    async fn fake_host(connections: Vec<Vec<Exchange>>) -> (Device, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let mut device = test_device();
        device.serial = "192.0.2.7:5555".to_string();
        device.host.host = Some("127.0.0.1".to_string());
        device.host.port = Some(listener.local_addr().unwrap().port());
        let task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3), async {
                for exchanges in connections {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    for (command, response) in exchanges {
                        let mut length = [0; 4];
                        stream.read_exact(&mut length).await.unwrap();
                        let length =
                            usize::from_str_radix(std::str::from_utf8(&length).unwrap(), 16)
                                .unwrap();
                        let mut actual = vec![0; length];
                        stream.read_exact(&mut actual).await.unwrap();
                        assert_eq!(String::from_utf8(actual).unwrap(), command);
                        stream.write_all(&response).await.unwrap();
                        if command.starts_with("shell,v2,raw:") {
                            let mut close_stdin = [0; 5];
                            stream.read_exact(&mut close_stdin).await.unwrap();
                            assert_eq!(close_stdin, [4, 0, 0, 0, 0]);
                        }
                    }
                }
            })
            .await
            .expect("fake ADB server timed out");
        });
        (device, task)
    }

    fn reply(message: &str) -> Vec<u8> {
        format!("OKAY{:04X}{message}", message.len()).into_bytes()
    }

    fn shell_exchanges(command: &str, exit_code: u8) -> Vec<Vec<Exchange>> {
        let mut response = b"OKAY".to_vec();
        response.extend_from_slice(&[2, 6, 0, 0, 0]);
        response.extend_from_slice(b"denied");
        response.extend_from_slice(&[3, 1, 0, 0, 0, exit_code]);
        vec![
            vec![(
                "host-serial:192.0.2.7:5555:features".into(),
                reply("shell_v2"),
            )],
            vec![
                ("host:transport:192.0.2.7:5555".into(), b"OKAY".to_vec()),
                (format!("shell,v2,raw:{command}"), response),
            ],
        ]
    }

    #[tokio::test]
    async fn shell_preserves_arguments_and_reports_remote_failure() {
        let (device, server) = fake_host(shell_exchanges(
            "'am' 'broadcast' '' '{\"name\":\"a b\"}' 'it'\\''s'",
            7,
        ))
        .await;
        let error = shell(
            &device,
            &["am", "broadcast", "", r#"{"name":"a b"}"#, "it's"],
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("exited with 7"));
        assert!(error.to_string().contains("denied"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn best_effort_shell_accepts_remote_failure() {
        let (device, server) = fake_host(shell_exchanges("'am' 'startservice'", 1)).await;
        shell_best_effort(&device, &["am", "startservice"])
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reverse_setup_and_cleanup_use_selected_device() {
        let (device, server) = fake_host(vec![
            vec![
                ("host:transport:192.0.2.7:5555".into(), b"OKAY".to_vec()),
                (
                    "reverse:forward:tcp:4445;tcp:4445".into(),
                    b"OKAYOKAY".to_vec(),
                ),
            ],
            vec![
                ("host:transport:192.0.2.7:5555".into(), b"OKAY".to_vec()),
                ("reverse:killforward:tcp:4445".into(), b"OKAYOKAY".to_vec()),
            ],
        ])
        .await;
        reverse(&device, 4445).await.unwrap();
        remove_reverse(&device, 4445).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reconnect_uses_selected_host_and_serial() {
        let (device, server) = fake_host(vec![vec![(
            "host-serial:192.0.2.7:5555:reconnect".into(),
            reply("reconnecting"),
        )]])
        .await;
        reconnect(&device).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn reconnect_reports_server_failure() {
        let (device, server) = fake_host(vec![vec![(
            "host-serial:192.0.2.7:5555:reconnect".into(),
            b"FAIL0007offline".to_vec(),
        )]])
        .await;
        assert!(
            reconnect(&device)
                .await
                .unwrap_err()
                .to_string()
                .contains("offline")
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn wait_ignores_other_devices_and_waits_for_selected_device() {
        let (device, server) = fake_host(vec![
            vec![(
                "host:devices-l".into(),
                reply("other device product:test\n192.0.2.7:5555 offline\n"),
            )],
            vec![(
                "host:devices-l".into(),
                reply("192.0.2.7:5555 device product:test\n"),
            )],
        ])
        .await;
        assert!(
            wait_for_device(&device, &AtomicBool::new(false))
                .await
                .unwrap()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn wait_can_be_cancelled_during_an_unresponsive_request() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let mut device = test_device();
        device.host.host = Some("127.0.0.1".to_string());
        device.host.port = Some(listener.local_addr().unwrap().port());
        let stop = AtomicBool::new(false);
        let cancel = async {
            let (_stream, _) = listener.accept().await.unwrap();
            stop.store(true, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        tokio::pin!(cancel);
        let wait = async {
            assert!(!wait_for_device(&device, &stop).await.unwrap());
        };
        tokio::select! {
            _ = &mut cancel => panic!("ADB wait did not stop promptly"),
            _ = wait => {},
        }
    }
}
