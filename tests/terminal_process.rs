#![cfg(target_os = "linux")]
mod support;
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    pty::{Winsize, openpty},
    sys::termios::{Termios, tcgetattr},
};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process {
    child: Child,
    master: File,
    slave: File,
    before: Termios,
    output: Vec<u8>,
    _home: tempfile::TempDir,
}
impl Process {
    fn launch(mut command: Command) -> Self {
        let pair = openpty(
            Some(&Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            None,
        )
        .unwrap();
        let master = File::from(pair.master);
        let slave = File::from(pair.slave);
        let before = tcgetattr(&slave).unwrap();
        let home = tempfile::tempdir().unwrap();
        command
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()))
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("TERM", "xterm-256color");
        // Only async-signal-safe syscalls run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                if nix::libc::ioctl(0, nix::libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        fcntl(&master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
        Self {
            child,
            master,
            slave,
            before,
            output: vec![],
            _home: home,
        }
    }
    fn drain(&mut self) {
        let mut buf = [0; 8192];
        loop {
            match self.master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.output.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("PTY read: {e}"),
            }
        }
    }
    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            self.drain();
            if String::from_utf8_lossy(&self.output).contains(needle) {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "child exited before {needle}: {}",
                String::from_utf8_lossy(&self.output)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "No {needle} in PTY output: {:?}",
            String::from_utf8_lossy(&self.output)
        );
    }
    fn finish(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.drain();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.drain();
                return status;
            }
            assert!(Instant::now() < deadline, "shutdown exceeded five seconds");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn restored(&mut self) {
        assert_eq!(tcgetattr(&self.slave).unwrap(), self.before);
        let out = String::from_utf8_lossy(&self.output);
        assert!(out.contains("\u{1b}[?1049l"));
        assert!(out.contains("\u{1b}[?25h"));
        assert!(out.contains("\u{1b}[?2004l"));
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whatsapp-tui"))
}
fn harness(mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "terminal_child", "--nocapture"])
        .env("WHATSAPP_TUI_TEST_CHILD", mode);
    c
}

#[test]
fn demo_restores_tty_after_exit() {
    let mut c = binary();
    c.arg("--demo");
    let mut p = Process::launch(c);
    p.wait_for("Alice");
    p.master.write_all(b"\x11").unwrap();
    assert!(p.finish().success());
    p.restored();
}
#[test]
fn demo_finds_chats_messages_and_unreads_then_restores_tty() {
    let mut command = binary();
    command.arg("--demo");
    let mut p = Process::launch(command);
    p.wait_for("Alice");
    p.master.write_all(b"\x10").unwrap();
    p.wait_for("Switch chat");
    p.master.write_all(b"\x1b[200~alc\x1b[201~").unwrap();
    p.wait_for("alc");
    p.master.write_all(b"\r").unwrap();
    p.master.write_all(b"\x06").unwrap();
    p.wait_for("search/open");
    p.master.write_all(b"\x1b[200~cyan\x1b[201~\r").unwrap();
    p.wait_for("matches");
    p.output.clear();
    p.master.write_all(b"\r\x1b[Zu").unwrap();
    p.wait_for("Unread");
    p.master.write_all(b"\x11").unwrap();
    assert!(p.finish().success());
    p.restored();
}
#[test]
fn controlled_panic_restores_tty() {
    let mut p = Process::launch(harness("panic"));
    assert!(!p.finish().success());
    p.restored();
    assert!(!String::from_utf8_lossy(&p.output).contains("PRIVATE_SENTINEL"));
}
#[test]
fn config_failure_precedes_raw_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[theme]\nfocus = 'not-a-color'\n").unwrap();
    let mut c = binary();
    c.args(["--demo", "--config"]).arg(path);
    let mut p = Process::launch(c);
    assert!(!p.finish().success());
    assert_eq!(tcgetattr(&p.slave).unwrap(), p.before);
    assert!(!String::from_utf8_lossy(&p.output).contains("\u{1b}[?1049h"));
}
#[test]
fn saturated_shutdown_finishes() {
    let mut p = Process::launch(harness("saturated"));
    assert!(
        p.finish().success(),
        "{}",
        String::from_utf8_lossy(&p.output)
    );
    p.restored();
}

#[test]
fn terminal_child() {
    let Ok(mode) = std::env::var("WHATSAPP_TUI_TEST_CHILD") else {
        return;
    };
    let _guard = whatsapp_tui::terminal::TerminalGuard::enter().unwrap();
    if mode == "panic" {
        panic!("PRIVATE_SENTINEL");
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        use support::*;
        use whatsapp_tui::{
            app::ViewModel,
            config::Config,
            runtime::{self, Screen},
            storage::Store,
            whatsapp::*,
        };
        struct Sink;
        impl Screen for Sink {
            fn draw(&mut self, _: &ViewModel, _: &Config) -> std::io::Result<()> {
                Ok(())
            }
            fn finish(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path().join("db")).await.unwrap();
        let (commands, mut requests) = tokio::sync::mpsc::channel(1);
        commands
            .send(BackendCommand::MarkRead(vec![]))
            .await
            .unwrap();
        let (tx, events) = tokio::sync::mpsc::channel(1);
        let (stop, wait) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let producer = tokio::spawn(async move {
                for n in 0..512 {
                    tx.send(BackendEvent::SendOutcome {
                        key: key("chat", "test", &n.to_string()),
                        state: whatsapp_tui::app::model::SendState::Sent,
                    })
                    .await
                    .unwrap();
                }
            });
            let _ = wait.await;
            requests.close();
            while requests.recv().await.is_some() {}
            producer.await.unwrap();
            Ok(())
        });
        let backend = BackendHandle {
            commands,
            events,
            control: BackendControl::new(stop, task),
        };
        let input = futures_util::stream::iter([Ok(crossterm::event::Event::Key(
            whatsapp_tui::config::bindings::parse_key("ctrl-q").unwrap(),
        ))]);
        tokio::time::timeout(
            Duration::from_secs(4),
            runtime::run_with_screen(ready_app(), store, backend, &mut Sink, input),
        )
        .await
        .expect("blocked shutdown")
        .unwrap();
        assert!(
            !BackendError::Service(anyhow::anyhow!("PRIVATE_SENTINEL"))
                .to_string()
                .contains("PRIVATE_SENTINEL")
        );
    });
}
