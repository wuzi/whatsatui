use std::{io, process::Stdio};

#[cfg(unix)]
pub(super) type ControlStream = tokio::net::UnixStream;
#[cfg(windows)]
pub(super) type ControlStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(unix)]
pub(super) async fn open() -> io::Result<(ControlStream, Stdio)> {
    let (parent, child) = std::os::unix::net::UnixStream::pair()?;
    parent.set_nonblocking(true)?;
    Ok((
        ControlStream::from_std(parent)?,
        Stdio::from(std::os::fd::OwnedFd::from(child)),
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        process::Command,
    };

    #[tokio::test]
    async fn inherited_pipe_is_duplex_and_closes_with_the_child() {
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            let (mut client, input) = open().await.unwrap();
            let mut child = Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    include_str!("../../../tests/fixtures/windows_pipe_peer.ps1"),
                ])
                .stdin(input)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let message = "{\"command\":[\"set_property\",\"title\",\"café $(throw 'bad')\"]}\n";
            client.write_all(message.as_bytes()).await.unwrap();
            let mut reader = BufReader::new(client);
            let mut reply = String::new();
            reader.read_line(&mut reply).await.unwrap();
            assert_eq!(reply.trim_end(), message.trim_end());
            assert!(child.wait().await.unwrap().success());
            reply.clear();
            assert_eq!(reader.read_line(&mut reply).await.unwrap(), 0);
        })
        .await
        .expect("IPC peer must finish and release its inherited handle");
    }
}

#[cfg(windows)]
pub(super) async fn open() -> io::Result<(ControlStream, Stdio)> {
    use crate::storage::paths::windows::SecurityDescriptor;
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        sync::atomic::{AtomicU64, Ordering},
    };
    use tokio::net::windows::named_pipe::ClientOptions;
    use windows_sys::Win32::{
        Foundation::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE},
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
        },
        System::{
            IO::{GetOverlappedResult, OVERLAPPED},
            Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS},
            Threading::CreateEventW,
        },
    };

    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        r"\\.\pipe\whatsapp-tui-mpv-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    // The inherited server must never be registered with Tokio's IOCP or issue
    // reads in the parent: it belongs to mpv's I/O loop. Only the client uses
    // Tokio. See https://learn.microsoft.com/windows/win32/fileio/createiocompletionport.
    let server = {
        let descriptor = SecurityDescriptor::current_user()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: 0,
        };
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: Name and security descriptor outlive the call. The returned
        // handle is uniquely owned, and only the current user can connect.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        unsafe { OwnedHandle::from_raw_handle(handle) }
    };
    let client = ClientOptions::new().open(&name)?;
    // SAFETY: Handles and OVERLAPPED storage remain alive until the connection
    // completes. The client was already opened, so ERROR_PIPE_CONNECTED is the
    // expected success case; pending operations are completed before returning.
    unsafe {
        let event = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = OwnedHandle::from_raw_handle(event);
        let mut overlapped = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        };
        if ConnectNamedPipe(server.as_raw_handle(), &mut overlapped) == 0 {
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_PIPE_CONNECTED) => {}
                Some(ERROR_IO_PENDING) => {
                    let mut bytes = 0;
                    if GetOverlappedResult(server.as_raw_handle(), &overlapped, &mut bytes, 1) == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                _ => return Err(error),
            }
        }
    }
    // Stdio duplicates this server handle into the child as inheritable stdin.
    // mpv wraps stdin's Windows handle as fd 0 and owns that side of the pipe.
    Ok((client, Stdio::from(server)))
}
