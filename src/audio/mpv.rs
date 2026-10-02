use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf},
    process::{Child, Command},
};
mod channel;
use channel::ControlStream;

#[derive(Default)]
pub(super) struct State {
    pub loaded: bool,
    pub paused: bool,
    pub position: u64,
    pub duration: Option<u64>,
    pub speed_milli: u32,
    pub finished: bool,
}
pub(super) struct Mpv {
    _child: Child,
    reader: BufReader<ReadHalf<ControlStream>>,
    writer: WriteHalf<ControlStream>,
    pending: Vec<u8>,
    serial: u64,
    video: bool,
    pub state: State,
}
impl Mpv {
    pub async fn start(executable: &Path, path: &Path, video: bool) -> Result<Self, String> {
        let (stream, input) = channel::open()
            .await
            .map_err(|_| "Cannot create playback control channel")?;
        // FD 0 is a private duplex socket/pipe. mpv exits when its
        // IPC client disconnects, including an unexpected parent process exit.
        let mut command = Command::new(executable);
        if video {
            // Restrict fallback to GUI outputs: a failed window must not write
            // video graphics into our terminal or fall back to a null output.
            #[cfg(unix)]
            command.arg("--vo=gpu-next,gpu,wlshm,x11");
            #[cfg(windows)]
            command.arg("--vo=gpu-next,gpu");
            command.args([
                "--force-window=yes",
                "--title=WhatsAppTUI video",
                "--osc=yes",
            ]);
        } else {
            command.arg("--no-video");
        }
        let child = command
            .args([
                "--no-config",
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
            .stdin(input)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Media player missing: install mpv or set [audio].player"
                } else {
                    "Cannot start mpv media player"
                }
            })?;
        let (reader, writer) = tokio::io::split(stream);
        let mut player = Self {
            _child: child,
            reader: BufReader::new(reader),
            writer,
            pending: Vec::new(),
            serial: 0,
            video,
            state: State {
                paused: true,
                speed_milli: 1000,
                ..Default::default()
            },
        };
        for (id, name) in [(1, "time-pos"), (2, "duration"), (3, "pause"), (4, "speed")] {
            player
                .command(json!(["observe_property", id, name]))
                .await?;
        }
        Ok(player)
    }
    pub async fn command(&mut self, command: Value) -> Result<(), String> {
        if self.state.finished {
            return Ok(());
        }
        self.serial += 1;
        let id = self.serial;
        let mut bytes = serde_json::to_vec(&json!({"command":command,"request_id":id}))
            .map_err(|_| "Invalid playback command")?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(2), async {
            if self.writer.write_all(&bytes).await.is_err() {
                // Closing a window may close its read side before we consume
                // the queued end-file event. Drain it within this command's
                // deadline before classifying the closed channel as a failure.
                while !self.state.finished {
                    self.read().await?;
                }
                return Ok(());
            }
            loop {
                let value = self.read().await?;
                // EOF can arrive while a health query/control is in flight.
                // Let the session publish Finished without waiting for a reply
                // from the player that is now exiting normally.
                if self.state.finished {
                    return Ok(());
                }
                if value["request_id"].as_u64() == Some(id) {
                    return if value["error"] == "success" {
                        Ok(())
                    } else {
                        Err("Media player rejected a playback control".into())
                    };
                }
            }
        })
        .await
        .map_err(|_| "Media player did not respond; try playing again")?
    }
    pub async fn read(&mut self) -> Result<Value, String> {
        loop {
            // fill_buf + consume is cancellation safe; unfinished lines remain
            // owned here when a control or validation tick wins select!.
            let bytes = self
                .reader
                .fill_buf()
                .await
                .map_err(|_| "Media player stopped unexpectedly")?;
            if bytes.is_empty() {
                return Err("Media player stopped; check the media file and output device".into());
            }
            let newline = bytes.iter().position(|b| *b == b'\n');
            let count = newline.map_or(bytes.len(), |i| i + 1);
            if self.pending.len() + count > 64 * 1024 {
                return Err("Media player returned an oversized control message".into());
            }
            self.pending.extend_from_slice(&bytes[..count]);
            self.reader.consume(count);
            if newline.is_some() {
                let value: Value = serde_json::from_slice(&std::mem::take(&mut self.pending))
                    .map_err(|_| "Media player returned invalid control data")?;
                self.apply(&value)?;
                return Ok(value);
            }
        }
    }
    fn apply(&mut self, value: &Value) -> Result<(), String> {
        match value["event"].as_str() {
            Some("file-loaded") => self.state.loaded = true,
            Some("end-file")
                if value["reason"] == "eof"
                    || (self.video
                        && matches!(value["reason"].as_str(), Some("quit" | "stop"))) =>
            {
                self.state.finished = true
            }
            Some("shutdown") if self.video => self.state.finished = true,
            Some("end-file") => {
                return Err("Cannot play this media; check its format and output device".into());
            }
            Some("property-change") => match value["name"].as_str() {
                Some("time-pos") => {
                    if let Some(ms) = millis(&value["data"]) {
                        self.state.position = ms;
                        self.state.loaded = true;
                    }
                }
                Some("duration") => self.state.duration = millis(&value["data"]),
                Some("speed") => {
                    if let Some(speed) = value["data"]
                        .as_f64()
                        .filter(|s| s.is_finite() && (0.01..=100.0).contains(s))
                    {
                        self.state.speed_milli = (speed * 1000.0).round() as u32;
                    }
                }
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
