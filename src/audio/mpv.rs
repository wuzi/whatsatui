use serde_json::{Value, json};
use std::{os::fd::OwnedFd, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    process::{Child, Command},
};

#[derive(Default)]
pub(super) struct State {
    pub loaded: bool,
    pub paused: bool,
    pub position: u64,
    pub duration: Option<u64>,
    pub finished: bool,
}
pub(super) struct Mpv {
    _child: Child,
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
    pending: Vec<u8>,
    serial: u64,
    pub state: State,
}
impl Mpv {
    pub async fn start(executable: &Path, path: &Path) -> Result<Self, String> {
        let (parent, child) = std::os::unix::net::UnixStream::pair()
            .map_err(|_| "Cannot create audio control channel")?;
        parent
            .set_nonblocking(true)
            .map_err(|_| "Cannot configure audio control channel")?;
        // FD 0 is a private duplex socket, not the terminal. mpv exits when its
        // IPC client disconnects, including an unexpected parent process exit.
        let child = Command::new(executable)
            .args([
                "--no-config",
                "--no-video",
                "--audio-display=no",
                "--terminal=no",
                "--input-terminal=no",
                "--input-ipc-client=fd://0",
                "--pause=yes",
                "--keep-open=no",
                "--idle=no",
                "--load-scripts=no",
                "--ytdl=no",
                "--access-references=no",
                "--sub-auto=no",
                "--audio-file-auto=no",
                "--demuxer-lavf-o=protocol_whitelist=file",
                "--audio-pitch-correction=yes",
                "--audio-client-name=WhatsAppTUI",
                "--",
            ])
            .arg(path)
            .stdin(Stdio::from(OwnedFd::from(child)))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Audio player missing: install mpv or set [audio].player"
                } else {
                    "Cannot start mpv audio player"
                }
            })?;
        let (reader, writer) = UnixStream::from_std(parent)
            .map_err(|_| "Cannot open audio control channel")?
            .into_split();
        let mut player = Self {
            _child: child,
            reader: BufReader::new(reader),
            writer,
            pending: Vec::new(),
            serial: 0,
            state: State {
                paused: true,
                ..Default::default()
            },
        };
        for (id, name) in [(1, "time-pos"), (2, "duration"), (3, "pause")] {
            player
                .command(json!(["observe_property", id, name]))
                .await?;
        }
        Ok(player)
    }
    pub async fn command(&mut self, command: Value) -> Result<(), String> {
        self.serial += 1;
        let id = self.serial;
        let mut bytes = serde_json::to_vec(&json!({"command":command,"request_id":id}))
            .map_err(|_| "Invalid audio command")?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(2), async {
            self.writer
                .write_all(&bytes)
                .await
                .map_err(|_| "Audio player closed its control channel")?;
            loop {
                let value = self.read().await?;
                if value["request_id"].as_u64() == Some(id) {
                    return if value["error"] == "success" {
                        Ok(())
                    } else {
                        Err("Audio player rejected a playback control".into())
                    };
                }
            }
        })
        .await
        .map_err(|_| "Audio player did not respond; try playing again")?
    }
    pub async fn read(&mut self) -> Result<Value, String> {
        loop {
            // fill_buf + consume is cancellation safe; unfinished lines remain
            // owned here when a control or validation tick wins select!.
            let bytes = self
                .reader
                .fill_buf()
                .await
                .map_err(|_| "Audio player stopped unexpectedly")?;
            if bytes.is_empty() {
                return Err("Audio player stopped; check the audio file and output device".into());
            }
            let newline = bytes.iter().position(|b| *b == b'\n');
            let count = newline.map_or(bytes.len(), |i| i + 1);
            if self.pending.len() + count > 64 * 1024 {
                return Err("Audio player returned an oversized control message".into());
            }
            self.pending.extend_from_slice(&bytes[..count]);
            self.reader.consume(count);
            if newline.is_some() {
                let value: Value = serde_json::from_slice(&std::mem::take(&mut self.pending))
                    .map_err(|_| "Audio player returned invalid control data")?;
                self.apply(&value)?;
                return Ok(value);
            }
        }
    }
    fn apply(&mut self, value: &Value) -> Result<(), String> {
        match value["event"].as_str() {
            Some("file-loaded") => self.state.loaded = true,
            Some("end-file") if value["reason"] == "eof" => self.state.finished = true,
            Some("end-file") => {
                return Err("Cannot play this audio; check its format and audio output".into());
            }
            Some("property-change") => match value["name"].as_str() {
                Some("time-pos") => {
                    if let Some(ms) = millis(&value["data"]) {
                        self.state.position = ms;
                        self.state.loaded = true;
                    }
                }
                Some("duration") => self.state.duration = millis(&value["data"]),
                Some("pause") => {
                    if let Some(paused) = value["data"].as_bool() {
                        self.state.paused = paused;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }
}
fn millis(value: &Value) -> Option<u64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= u32::MAX as f64)
        .map(|n| (n * 1000.0).round() as u64)
}
