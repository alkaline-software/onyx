//! A terminal with no window: the person's shell on a pty this process
//! owns, and a screen model with no renderer (vt100, `vendor/vt100`) that
//! keeps what it draws. `rows_back` is that screen as colored stretches
//! for the window to draw, and `write` is the keyboard. Taken from
//! Emaki's, its shell terminal's part. `docs/window.md`, The terminal.
//!
//! The one child process not started through `process::command`: a pty
//! is portable-pty's to open, and the command is its `CommandBuilder`.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};


/// The screen's size until the window gives it its own.
pub const ROWS: u16 = 24;
pub const COLS: u16 = 80;
/// How many rows that have left the top of the screen are kept, for the
/// terminal to scroll back through.
pub const SCROLLBACK: usize = 5000;

/// A stretch of one row drawn the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// `0xRRGGBB`; None is the terminal's own ink or ground.
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    /// Which of the sixteen named colors the ink or the ground is, when
    /// the program named one: a terminal draws those in its own palette.
    pub fg_ix: Option<u8>,
    pub bg_ix: Option<u8>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    /// Where each of its cells begins in `text`, and how many columns it
    /// takes, as the screen model laid them out. A cell may hold more
    /// than one character (a letter and its accent, an emoji and its
    /// variation selector), and which are wide is the model's to say:
    /// counted again by a table of our own, an emoji the table did not
    /// have put everything after it a column off where the cursor was.
    pub cells: Vec<(usize, u8)>,
}

impl Span {
    /// Its cells, each what it holds and how many columns it takes. One
    /// made with no cells is a column a character.
    pub fn each_cell(&self) -> Vec<(&str, u8)> {
        if self.cells.is_empty() {
            return self.text.char_indices().map(|(at, c)| (&self.text[at..at + c.len_utf8()], 1)).collect();
        }
        (0..self.cells.len())
            .map(|i| {
                let (at, w) = self.cells[i];
                let end = self.cells.get(i + 1).map(|c| c.0).unwrap_or(self.text.len());
                (self.text.get(at..end).unwrap_or(""), w)
            })
            .collect()
    }

    fn same_look(&self, o: &Span) -> bool {
        self.fg == o.fg && self.bg == o.bg && self.fg_ix == o.fg_ix && self.bg_ix == o.bg_ix && self.bold == o.bold && self.dim == o.dim && self.italic == o.italic && self.underline == o.underline && self.inverse == o.inverse
    }
}

pub struct Pty {
    pub pid: i32,
    parser: Mutex<vt100::Parser>,
    /// Whether a resize reflows its lines (`reflow_on_resize`).
    reflows: std::sync::atomic::AtomicBool,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    alive: AtomicBool,
    last_output: Mutex<Instant>,
    /// What the program last asked to have put on the clipboard (OSC
    /// 52), until the window takes it.
    copied: Mutex<Option<String>>,
}

/// What a program hears of the mouse: a button let go, the pointer
/// moving with a button held, the pointer moving at all, and the form
/// the reports take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseWant {
    pub release: bool,
    pub drag: bool,
    pub motion: bool,
    pub form: MouseForm,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseForm {
    /// `ESC [ M` and three bytes: nothing past column or row 223.
    Bytes,
    /// The same, each number a UTF-8 character.
    Utf8,
    /// `ESC [ < code ; column ; row` and `M`, or `m` for a button let go.
    Sgr,
}

/// One report of the mouse, as the bytes a terminal sends. `code` is the
/// button (0 left, 1 middle, 2 right; 64 and 65 the wheel up and down),
/// with 32 added while the pointer moves and 4, 8, 16 for Shift, Alt and
/// Control. The cell is counted from 0. Empty when the form cannot say it.
pub fn mouse_bytes(code: u8, col: u16, row: u16, press: bool, form: MouseForm) -> Vec<u8> {
    match form {
        MouseForm::Sgr => format!("\x1b[<{code};{};{}{}", col + 1, row + 1, if press { 'M' } else { 'm' }).into_bytes(),
        MouseForm::Bytes | MouseForm::Utf8 => {
            // A button let go is button 3 in these forms, the modifiers kept.
            let code = if press { code } else { code & !3 | 3 };
            let (b, x, y) = (32 + code as u32, 33 + col as u32, 33 + row as u32);
            let mut out = b"\x1b[M".to_vec();
            for v in [b, x, y] {
                if form == MouseForm::Utf8 {
                    let Some(c) = char::from_u32(v) else { return Vec::new() };
                    out.extend(c.to_string().as_bytes());
                } else if v <= 255 {
                    out.push(v as u8);
                } else {
                    return Vec::new();
                }
            }
            out
        }
    }
}

/// The person's own shell, as a login shell, for the terminal: what
/// `SHELL` names, else `sh`. On Windows PowerShell, as Windows Terminal
/// opens: PowerShell 7 where it is installed, else the Windows PowerShell
/// every Windows has.
pub fn shell_argv() -> Vec<String> {
    if cfg!(windows) {
        static PWSH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let shell = PWSH.get_or_init(|| {
            let seven = std::env::var_os("ProgramFiles").map(|p| std::path::PathBuf::from(p).join("PowerShell").join("7"));
            let dirs = windows_path_now().map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
            let five = std::env::var_os("SystemRoot").map(|r| std::path::PathBuf::from(r).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"));
            dirs.into_iter().chain(seven).map(|d| d.join("pwsh.exe")).find(|p| p.is_file()).or(five).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "powershell.exe".into())
        });
        return vec![shell.clone(), "-NoLogo".into()];
    }
    vec![std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into()), "-l".into()]
}

/// The environment for the child: the app's own, as a terminal that is
/// nobody's in particular. The app may have been started from a terminal
/// app, and a program reads that app's marks to decide which keyboard
/// protocol to speak; here it gets the plain one. What a launch was told
/// (`ONYX_*`, `docs/probing.md`) is that launch's alone: handed on, a
/// relaunch from the terminal would open the probe's vault for ever.
fn child_env() -> Vec<(String, String)> {
    const THEIRS: &[&str] = &["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "TERM_SESSION_ID", "LC_TERMINAL", "LC_TERMINAL_VERSION", "TMUX", "TMUX_PANE", "STY", "NO_COLOR", "FORCE_COLOR", "COLUMNS", "LINES", "__CFBundleIdentifier"];
    const THEIR_PREFIXES: &[&str] = &["WEZTERM_", "KITTY_", "ITERM_", "VSCODE_", "KAKU_", "GHOSTTY_", "ALACRITTY_", "KONSOLE_", "WT_"];
    let mut env: Vec<(String, String)> =
        std::env::vars().filter(|(k, _)| !k.starts_with("ONYX_")).filter(|(k, _)| !THEIRS.contains(&k.as_str()) && !THEIR_PREFIXES.iter().any(|p| k.starts_with(p))).collect();
    env.push(("TERM".into(), "xterm-256color".into()));
    // An app opened from the Finder has no locale, and a shell with none
    // counts a letter of more than a byte (é, 世) as that many columns: the
    // line it draws again puts the cursor off its letters. Characters are
    // UTF-8, as a Mac's Terminal sets them; the language is left alone.
    if !env.iter().any(|(k, v)| matches!(k.as_str(), "LANG" | "LC_ALL" | "LC_CTYPE") && !v.is_empty()) {
        env.push(("LC_CTYPE".into(), if cfg!(target_os = "macos") { "UTF-8" } else { "C.UTF-8" }.into()));
    }
    env.push(("COLORTERM".into(), "truecolor".into()));
    // On Windows a program installed since the app started is on the
    // PATH Windows keeps, not on the app's: a new terminal gets the one
    // kept now, as Windows Terminal's new tab does.
    if let Some(path) = windows_path_now() {
        env.retain(|(k, _)| !k.eq_ignore_ascii_case("PATH"));
        env.push(("PATH".into(), path));
    }
    env
}

/// The newest whole "set the clipboard" sequence in `pending` (OSC 52:
/// `ESC ] 52 ; <which> ; <base64>` ended by BEL or `ESC \`), decoded, and
/// `pending` cut down to what may still be the start of one. A program
/// run in a terminal copies this way when it draws its own selection,
/// as an editor in the terminal may, and a terminal that ignores it leaves "copied"
/// on the screen and nothing on the clipboard. A question (`?`) asks
/// what the clipboard holds, and is not answered.
pub fn take_osc52(pending: &mut Vec<u8>) -> Option<String> {
    const START: &[u8] = b"\x1b]52;";
    const MOST: usize = 4 << 20;
    let mut found = None;
    loop {
        let Some(at) = pending.windows(START.len()).position(|w| w == START) else {
            // Nothing begun, but the last bytes may be the start of one.
            let keep = pending.len().saturating_sub(START.len() - 1);
            pending.drain(..keep);
            return found;
        };
        pending.drain(..at);
        let body = &pending[START.len()..];
        let Some(end) = body.iter().position(|b| *b == 0x07 || *b == 0x1b) else {
            if pending.len() > MOST {
                pending.clear();
            }
            return found;
        };
        let data = body[..end].splitn(2, |b| *b == b';').nth(1).unwrap_or(&[]);
        if let Some(text) = std::str::from_utf8(data).ok().filter(|d| *d != "?").and_then(base64_decode).and_then(|b| String::from_utf8(b).ok()) {
            found = Some(text);
        }
        pending.drain(..START.len() + end + 1);
    }
}

impl Pty {
    /// Start `argv` in `cwd` on a pty of its own. `on_output` is called
    /// from the reading thread whenever the screen changed, and once more
    /// when the child is gone.
    pub fn spawn(argv: &[String], cwd: &str, on_output: Arc<dyn Fn() + Send + Sync>) -> Result<Arc<Pty>, String> {
        Self::spawn_with(argv, &[], cwd, on_output)
    }

    /// `spawn`, with more in the child's environment (`shell_marks`).
    pub fn spawn_with(argv: &[String], more_env: &[(String, String)], cwd: &str, on_output: Arc<dyn Fn() + Send + Sync>) -> Result<Arc<Pty>, String> {
        let (bin, rest) = argv.split_first().ok_or("nothing to run")?;
        let pair = native_pty_system().openpty(PtySize { rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(bin);
        cmd.args(rest);
        cmd.cwd(cwd);
        cmd.env_clear();
        for (k, v) in child_env().into_iter().chain(more_env.iter().cloned()) {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let pid = child.process_id().ok_or("the child has no pid")? as i32;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let pty = Arc::new(Pty {
            pid,
            parser: Mutex::new(vt100::Parser::new(ROWS, COLS, SCROLLBACK)),
            reflows: std::sync::atomic::AtomicBool::new(false),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(child.clone_killer()),
            alive: AtomicBool::new(true),
            last_output: Mutex::new(Instant::now()),
            copied: Mutex::new(None),
        });
        let log = std::env::var_os("ONYX_PTY_LOG").and_then(|p| std::fs::File::create(p).ok());
        let me = Arc::clone(&pty);
        std::thread::Builder::new()
            .name("onyx-pty".into())
            .spawn(move || {
                let mut log = log;
                let mut buf = [0u8; 8192];
                // What a query was cut off at, at the end of the last read.
                let mut tail: Vec<u8> = Vec::new();
                // A copy the program asks for, which may come in pieces.
                let mut osc: Vec<u8> = Vec::new();
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if let Some(f) = log.as_mut() {
                        let _ = f.write_all(&buf[..n]);
                    }
                    me.feed(&buf[..n]);
                    *held(&me.last_output) = Instant::now();
                    tail.extend_from_slice(&buf[..n]);
                    me.answer_queries(&tail);
                    osc.extend_from_slice(&buf[..n]);
                    if let Some(text) = take_osc52(&mut osc) {
                        *held(&me.copied) = Some(text);
                    }
                    let keep = tail.len().saturating_sub(8);
                    tail.drain(..keep);
                    // A sequence answered once is not kept to be answered again.
                    if tail.ends_with(b"n") || tail.ends_with(b"c") {
                        tail.clear();
                    }
                    on_output();
                }
                me.alive.store(false, Ordering::SeqCst);
                on_output();
            })
            .map_err(|e| e.to_string())?;
        // The child is waited for, so it leaves no zombie behind.
        let me = Arc::clone(&pty);
        std::thread::spawn(move || {
            let _ = child.wait();
            me.alive.store(false, Ordering::SeqCst);
        });
        Ok(pty)
    }

    /// What a program asks its terminal and waits on: where the cursor
    /// is (`ESC [ 6 n`) and what kind of terminal this is (`ESC [ c`).
    /// The screen model answers nothing by itself.
    fn answer_queries(&self, recent: &[u8]) {
        let has = |needle: &[u8]| recent.windows(needle.len()).any(|w| w == needle);
        if has(b"\x1b[6n") {
            let (row, col) = held(&self.parser).screen().cursor_position();
            self.write(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
        }
        if has(b"\x1b[c") || has(b"\x1b[0c") {
            self.write(b"\x1b[?62;c");
        }
    }

    /// The text the program asked to have copied since this was last
    /// asked, once. A terminal with no window has no clipboard of its
    /// own: the window puts it on the system's.
    pub fn take_copied(&self) -> Option<String> {
        held(&self.copied).take()
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// How long ago the screen last changed.
    pub fn quiet_for(&self) -> std::time::Duration {
        held(&self.last_output).elapsed()
    }

    /// Keys, as bytes.
    pub fn write(&self, bytes: &[u8]) {
        let mut w = held(&self.writer);
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    /// Words put in the prompt the way a paste arrives, so new lines in
    /// them stay new lines and send nothing.
    pub fn paste(&self, text: &str) {
        self.write(&self.paste_bytes(text));
    }

    /// What `paste` sends for these words.
    pub fn paste_bytes(&self, text: &str) -> Vec<u8> {
        if held(&self.parser).screen().bracketed_paste() {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace('\n', " ").into_bytes()
        }
    }

    /// From now on a resize reflows the lines, as a terminal window's
    /// does.
    pub fn reflow_on_resize(&self) {
        self.reflows.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        // Asked before the program hears of the new size.
        let keep = self.edits_its_line();
        let _ = held(&self.master).resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        let mut parser = held(&self.parser);
        let reflows = self.reflows.load(std::sync::atomic::Ordering::Relaxed);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| if reflows { parser.screen_mut().set_size_reflowed(rows, cols, keep) } else { parser.screen_mut().set_size(rows, cols) })).is_err() {
            *parser = vt100::Parser::new(rows, cols, SCROLLBACK);
        }
    }

    /// The program's output to the screen. The screen model is another's
    /// (vt100), and a panic in it here, on the thread that reads the pty,
    /// left its lock poisoned and the window's next look at the screen
    /// aborted the app: a screen that panics is begun again at its size,
    /// and the program's next drawing fills it.
    fn feed(&self, bytes: &[u8]) {
        let mut parser = held(&self.parser);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parser.process(bytes))).is_err() {
            let (rows, cols) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parser.screen().size())).unwrap_or((ROWS, COLS));
            *parser = vt100::Parser::new(rows, cols, SCROLLBACK);
        }
    }

    /// The screen's size: rows, columns.
    pub fn size(&self) -> (u16, u16) {
        held(&self.parser).screen().size()
    }

    /// Where the cursor is, row and column, unless the program hid it.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let parser = held(&self.parser);
        let screen = parser.screen();
        (!screen.hide_cursor()).then(|| screen.cursor_position())
    }

    /// Whether the program asked for the arrow keys in their application
    /// form (`ESC O A`), as a full-screen editor does.
    pub fn app_cursor(&self) -> bool {
        held(&self.parser).screen().application_cursor()
    }

    /// Whether the program reading the keys now edits its own line: a
    /// shell at its prompt, an agent's, Python's or node's prompt, a
    /// program in the shell as much as the shell. Each takes the terminal
    /// out of its line-at-a-time mode and takes the arrow keys a click or
    /// a deletion is made of. A command that reads whole lines, or none,
    /// leaves that mode on, and would print them. Off Unix, always.
    pub fn edits_its_line(&self) -> bool {
        #[cfg(unix)]
        {
            let Some(fd) = held(&self.master).as_raw_fd() else { return true };
            // SAFETY: a termios is plain data, filled by the call.
            let mut t: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(fd, &mut t) } != 0 {
                return true;
            }
            t.c_lflag & libc::ICANON == 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    }

    /// Where what is typed at the prompt begins, as the shell marked it
    /// (OSC 133 `B`, `shell_marks`): a line counted from the screen's top
    /// row, less than 0 for one that has left it, and a column.
    pub fn typing_start(&self) -> Option<(i64, u16)> {
        held(&self.parser).screen().typing_start()
    }

    /// Whether the program has the screen to itself (an editor, a
    /// pager), where there is nothing to scroll back to.
    pub fn alt_screen(&self) -> bool {
        held(&self.parser).screen().alternate_screen()
    }

    /// The screen as it was `back` rows ago, and how far back that
    /// really is (there may be fewer rows kept). The look is taken and
    /// the screen put back at once: the readers of `rows`, `text` and
    /// `styled` always see it as it is now.
    pub fn rows_back(&self, back: usize) -> (Vec<Vec<Span>>, usize) {
        let mut parser = held(&self.parser);
        parser.screen_mut().set_scrollback(back);
        let at = parser.screen().scrollback();
        let rows = rows_of(parser.screen());
        parser.screen_mut().set_scrollback(0);
        (rows, at)
    }

    /// Which rows of that look run on into the next, a long line and
    /// not two: copied, such rows are one line.
    pub fn wraps_back(&self, back: usize) -> Vec<bool> {
        let mut parser = held(&self.parser);
        parser.screen_mut().set_scrollback(back);
        let rows = parser.screen().size().0;
        let wraps = (0..rows).map(|r| parser.screen().row_wrapped(r)).collect();
        parser.screen_mut().set_scrollback(0);
        wraps
    }

    /// How many rows that have left the screen are kept, which is as far
    /// back as it scrolls.
    pub fn kept_back(&self) -> usize {
        let mut parser = held(&self.parser);
        parser.screen_mut().set_scrollback(usize::MAX);
        let kept = parser.screen().scrollback();
        parser.screen_mut().set_scrollback(0);
        kept
    }

    /// The lines from `from` to `to`, counted from the screen's top row
    /// as it is now and less than 0 for those that have left it, each
    /// with whether it runs on into the next. Held to the lines there
    /// are: the first is the line it begins at, `from` or the oldest kept.
    pub fn lines(&self, from: i64, to: i64) -> (i64, Vec<(Vec<Span>, bool)>) {
        lines_of(&mut held(&self.parser), from, to)
    }

    /// The screen and everything that has left it are forgotten, as a
    /// terminal's "clear" does, and what the program asked for (the
    /// mouse, pastes in brackets, the cursor) is kept. Not where a
    /// program has the screen to itself: `false` then, and nothing done.
    /// The program is not told; a redraw is the caller's to ask for.
    pub fn clear_all(&self) -> bool {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        let mut parser = held(&self.parser);
        let screen = parser.screen();
        if screen.alternate_screen() {
            return false;
        }
        let mut again: Vec<u8> = Vec::new();
        for (on, seq) in [
            (screen.bracketed_paste(), &b"\x1b[?2004h"[..]),
            (screen.application_cursor(), b"\x1b[?1h"),
            (screen.application_keypad(), b"\x1b="),
            (screen.hide_cursor(), b"\x1b[?25l"),
            (screen.mouse_protocol_mode() == M::Press, b"\x1b[?9h"),
            (screen.mouse_protocol_mode() == M::PressRelease, b"\x1b[?1000h"),
            (screen.mouse_protocol_mode() == M::ButtonMotion, b"\x1b[?1002h"),
            (screen.mouse_protocol_mode() == M::AnyMotion, b"\x1b[?1003h"),
            (screen.mouse_protocol_encoding() == E::Utf8, b"\x1b[?1005h"),
            (screen.mouse_protocol_encoding() == E::Sgr, b"\x1b[?1006h"),
        ] {
            if on {
                again.extend(seq);
            }
        }
        parser.process(b"\x1bc");
        parser.process(&again);
        true
    }

    /// What the program asked to be told of the mouse, when anything.
    pub fn mouse(&self) -> Option<MouseWant> {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        let parser = held(&self.parser);
        let screen = parser.screen();
        let mode = screen.mouse_protocol_mode();
        if mode == M::None {
            return None;
        }
        Some(MouseWant {
            release: mode != M::Press,
            drag: matches!(mode, M::ButtonMotion | M::AnyMotion),
            motion: mode == M::AnyMotion,
            form: match screen.mouse_protocol_encoding() {
                E::Sgr => MouseForm::Sgr,
                E::Utf8 => MouseForm::Utf8,
                E::Default => MouseForm::Bytes,
            },
        })
    }

    /// Let the child go, as closing a terminal's window does.
    pub fn kill(&self) {
        let _ = held(&self.killer).kill();
        self.alive.store(false, Ordering::SeqCst);
    }

}

/// xterm's 256 colors.
fn indexed(i: u8) -> u32 {
    const BASE: [u32; 16] = [
        0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666, 0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ];
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let i = i as u32 - 16;
            let level = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
            level(i / 36) << 16 | level(i / 6 % 6) << 8 | level(i % 6)
        }
        _ => {
            let v = 8 + 10 * (i as u32 - 232);
            v << 16 | v << 8 | v
        }
    }
}

fn rgb(c: vt100::Color) -> Option<u32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some((r as u32) << 16 | (g as u32) << 8 | b as u32),
    }
}

fn named(c: vt100::Color) -> Option<u8> {
    match c {
        vt100::Color::Idx(i) if i < 16 => Some(i),
        _ => None,
    }
}

/// `Pty::lines`, of a screen model.
fn lines_of(parser: &mut vt100::Parser, from: i64, to: i64) -> (i64, Vec<(Vec<Span>, bool)>) {
    let rows = parser.screen().size().0 as i64;
    parser.screen_mut().set_scrollback(usize::MAX);
    let kept = parser.screen().scrollback() as i64;
    let (mut at, to) = (from.max(-kept), to.min(rows - 1));
    let first = at;
    let mut out = Vec::new();
    while at <= to {
        // The look whose top row is `at`, or the screen itself.
        let back = (-at).max(0);
        parser.screen_mut().set_scrollback(back as usize);
        let shown = rows_of(parser.screen());
        for r in (at + back)..rows {
            if at > to {
                break;
            }
            out.push((shown[r as usize].clone(), parser.screen().row_wrapped(r as u16)));
            at += 1;
        }
    }
    parser.screen_mut().set_scrollback(0);
    (first, out)
}

/// A screen as stretches of text, a row at a time, blank to the right
/// of the last thing written.
pub fn rows_of(screen: &vt100::Screen) -> Vec<Vec<Span>> {
    let (rows, cols) = screen.size();
    (0..rows)
        .map(|r| {
            let mut out: Vec<Span> = Vec::new();
            for c in 0..cols {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let span = Span {
                    text: if cell.has_contents() { cell.contents().to_string() } else { " ".into() },
                    fg: rgb(cell.fgcolor()),
                    bg: rgb(cell.bgcolor()),
                    fg_ix: named(cell.fgcolor()),
                    bg_ix: named(cell.bgcolor()),
                    bold: cell.bold(),
                    dim: cell.dim(),
                    italic: cell.italic(),
                    underline: cell.underline(),
                    inverse: cell.inverse(),
                    cells: vec![(0, if cell.is_wide() { 2 } else { 1 })],
                };
                match out.last_mut() {
                    Some(last) if last.same_look(&span) => {
                        let base = last.text.len();
                        last.text.push_str(&span.text);
                        last.cells.push((base, span.cells[0].1));
                    }
                    _ => out.push(span),
                }
            }
            // Nothing drawn after the last mark: unstyled blanks go.
            while out.last().is_some_and(|s| s.bg.is_none() && !s.inverse && !s.underline && s.text.trim_end_matches(' ').is_empty()) {
                out.pop();
            }
            if let Some(last) = out.last_mut().filter(|s| s.bg.is_none() && !s.inverse && !s.underline) {
                let n = last.text.trim_end_matches(' ').len();
                last.text.truncate(n);
                last.cells.retain(|(at, _)| *at < n);
            }
            out
        })
        .collect()
}

/// What a screen of that size holds once `bytes` have been written to
/// it: the screen model without a pty, for a test.
#[cfg(test)]
pub fn rows_after(bytes: &[u8], rows: u16, cols: u16) -> Vec<Vec<Span>> {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(bytes);
    rows_of(parser.screen())
}

/// A web address on a screen, with the stretch of each row it takes:
/// row, start and end in characters. A program draws a long one as rows,
/// cut at the screen's edge with no mark, so an address that runs to the end of its row goes on with what
/// begins the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkRun {
    pub url: String,
    pub parts: Vec<(usize, usize, usize)>,
}

/// Every web address on the screen (`LinkRun`).
pub fn links(rows: &[Vec<Span>]) -> Vec<LinkRun> {
    let texts: Vec<Vec<char>> = rows.iter().map(|row| row.iter().flat_map(|s| s.text.chars()).collect::<String>().trim_end().chars().collect()).collect();
    let url_char = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
    let mut found = Vec::new();
    let mut r = 0;
    while r < texts.len() {
        let line = &texts[r];
        let mut i = 0;
        let mut next_row = r + 1;
        while i + 8 <= line.len() {
            let at: String = line[i..].iter().take(8).collect();
            if !(at.starts_with("https://") || at.starts_with("http://")) || (i > 0 && url_char(line[i - 1]) && line[i - 1] != '(') {
                i += 1;
                continue;
            }
            let end = line[i..].iter().position(|c| !url_char(*c)).map(|n| i + n).unwrap_or(line.len());
            let mut url: String = line[i..end].iter().collect();
            let mut parts = vec![(r, i, end)];
            // Run to its row's end: what begins the next row goes on with it.
            let (mut row, mut reached) = (r, end == line.len());
            while reached && row + 1 < texts.len() {
                let more = &texts[row + 1];
                let n = more.iter().position(|c| !url_char(*c)).unwrap_or(more.len());
                if n == 0 {
                    break;
                }
                row += 1;
                url.extend(more[..n].iter());
                parts.push((row, 0, n));
                reached = n == more.len();
            }
            // The stop after it is not part of it.
            while url.ends_with(['.', ',', ';', ':', '!', '?', ')', ']']) && !(url.ends_with(')') && url.contains('(')) {
                url.pop();
                if let Some(last) = parts.last_mut() {
                    last.2 -= 1;
                }
            }
            found.push(LinkRun { url, parts });
            if row > r {
                next_row = row + 1;
                break;
            }
            i = end;
        }
        r = next_row.max(r + 1);
    }
    found
}

/// The rows as plain text, a line a row.
pub fn plain(rows: &[Vec<Span>]) -> String {
    rows.iter().map(|row| row.iter().map(|s| s.text.as_str()).collect::<String>().trim_end_matches(' ').to_string()).collect::<Vec<_>>().join("\n")
}


/// A lock taken even when a thread that held it panicked: the screen is
/// begun again in that case (`feed`), and the window must not abort.
fn held<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The PATH as Windows keeps it now, for a terminal opened after an
/// install: none off Windows.
fn windows_path_now() -> Option<String> {
    None
}

/// Base64, standard or URL-safe, as OSC 52 carries the words to copy.
/// Whitespace is skipped; anything else invalid is none.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in text.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{base64_decode, mouse_bytes, plain, rows_after, take_osc52, MouseForm};

    #[test]
    fn a_copy_the_program_asks_for_is_read() {
        let mut pending = b"x\x1b]52;c;aGVsbG8=\x07y".to_vec();
        assert_eq!(take_osc52(&mut pending).as_deref(), Some("hello"));
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
    }

    #[test]
    fn the_screen_keeps_what_was_written() {
        let rows = rows_after(b"one\r\n\x1b[31mtwo\x1b[0m", 3, 10);
        assert_eq!(plain(&rows), "one\ntwo\n");
        assert_eq!(rows[1][0].fg_ix, Some(1));
    }

    #[test]
    fn a_click_is_told_in_the_form_asked_for() {
        assert_eq!(mouse_bytes(0, 2, 3, true, MouseForm::Sgr), b"\x1b[<0;3;4M");
        assert_eq!(mouse_bytes(0, 0, 0, false, MouseForm::Bytes), b"\x1b[M#!!");
    }
}
