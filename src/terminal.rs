use crate::runtime::AppError;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{self, IsTerminal, Write};
use std::sync::{
    Once,
    atomic::{AtomicBool, Ordering},
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();
pub struct TerminalGuard {
    active: bool,
}
impl TerminalGuard {
    pub fn enter() -> Result<Self, AppError> {
        Self::enter_with_mouse(true)
    }
    pub fn enter_with_mouse(mouse: bool) -> Result<Self, AppError> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(AppError::Arguments(
                "An interactive terminal is required; run --help for options",
            ));
        }
        PANIC_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if ACTIVE.swap(false, Ordering::SeqCst) {
                    let _ = restore();
                    eprintln!("whatsapp-tui stopped unexpectedly; the terminal has been restored.");
                } else {
                    previous(info);
                }
            }));
        });
        enable_raw_mode()?;
        ACTIVE.store(true, Ordering::SeqCst);
        let guard = Self { active: true };
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            ),
            EnableBracketedPaste,
            EnableFocusChange,
            Hide
        )?;
        if mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        Ok(guard)
    }
    pub fn restore(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            // The panic hook may already have restored the terminal. Claim
            // cleanup once so unwinding cannot pop the caller's keyboard mode.
            if ACTIVE.swap(false, Ordering::SeqCst) {
                return restore();
            }
        }
        Ok(())
    }
}
fn restore() -> io::Result<()> {
    let mut out = io::stdout();
    let result = execute!(
        out,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture,
        PopKeyboardEnhancementFlags,
        Show,
        LeaveAlternateScreen
    );
    let raw = disable_raw_mode();
    let flush = out.flush();
    result.and(raw).and(flush)
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
