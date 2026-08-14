use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor;
use crossterm::style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use unicode_width::UnicodeWidthChar;

use crate::history::{self, HistoryEntry, SearchResult};
use crate::input::{self, Key};
use crate::labels::Label;
use crate::stats::Stats;

use std::collections::HashMap;

// ── SIGWINCH handling ──────────────────────────────────────────────────

static RESIZED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigwinch(_: libc::c_int) {
    RESIZED.store(true, Ordering::Relaxed);
}

// ── Colors ──────────────────────────────────────────────────────────────

const COLOR_TEXT: Color = Color::Rgb { r: 220, g: 220, b: 220 };
const COLOR_MATCH: Color = Color::Rgb { r: 255, g: 120, b: 100 };
const COLOR_BORDER: Color = Color::Rgb { r: 100, g: 100, b: 100 };
const COLOR_TITLE: Color = Color::Rgb { r: 180, g: 180, b: 180 };
const COLOR_DIM: Color = Color::Rgb { r: 90, g: 90, b: 90 };
const COLOR_SELECTED_BG: Color = Color::Rgb { r: 60, g: 60, b: 65 };
const COLOR_CURSOR: Color = Color::Rgb { r: 200, g: 200, b: 200 };
const COLOR_MULTI: Color = Color::Rgb { r: 255, g: 200, b: 60 };
const COLOR_WARN_BG: Color = Color::Rgb { r: 120, g: 40, b: 40 };
const COLOR_WARN_FG: Color = Color::Rgb { r: 255, g: 220, b: 220 };

// ── TTY helpers ─────────────────────────────────────────────────────────

/// Newtype to provide Write without Read trait ambiguity on File.
struct TtyOut(File);

impl Write for TtyOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.0.write(buf) }
    fn flush(&mut self) -> io::Result<()> { self.0.flush() }
}

fn tty_size(fd: i32) -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col, ws.ws_row)
        } else {
            (80, 24)
        }
    }
}

/// Set VMIN/VTIME on the tty fd for read timeout behavior.
/// VMIN=0, VTIME=t → read() returns 0 after t*100ms if no byte arrives.
fn set_read_timeout(fd: i32, vtime: u8) {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut termios);
        termios.c_cc[libc::VMIN] = 0;
        termios.c_cc[libc::VTIME] = vtime;
        libc::tcsetattr(fd, libc::TCSANOW, &termios);
    }
}

/// Restore VMIN=1, VTIME=0 (blocking read, no timeout).
fn set_read_blocking(fd: i32) {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut termios);
        termios.c_cc[libc::VMIN] = 1;
        termios.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &termios);
    }
}

// ── Display width ───────────────────────────────────────────────────────

fn char_width(ch: char) -> usize {
    if ch.is_control() { 0 } else { UnicodeWidthChar::width(ch).unwrap_or(1) }
}

fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}


fn truncate_str(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = char_width(ch);
        if w + cw > max_width { result.push('…'); break; }
        result.push(ch);
        w += cw;
    }
    result
}

// ── Non-blocking key reading ───────────────────────────────────────────

/// Try to read a key with VTIME timeout.
/// Returns None if read timed out (no input within ~100ms).
fn try_read_key(tty_r: &mut File) -> io::Result<Option<Key>> {
    match input::read_key(tty_r) {
        Ok(key) => Ok(Some(key)),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

// ── Entry point ─────────────────────────────────────────────────────────

pub fn run(
    entries: Vec<HistoryEntry>,
    mut stats_data: Option<&mut Stats>,
    label_map: &HashMap<String, Label>,
    selection_counts: &HashMap<String, u32>,
) -> io::Result<Option<String>> {
    let mut tty_r = File::options().read(true).open("/dev/tty")?;
    let tty_fd = tty_r.as_raw_fd();
    let mut tty_w = TtyOut(File::options().write(true).open("/dev/tty")?);

    // Register SIGWINCH handler (sa_flags=0: no SA_RESTART)
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    sa.sa_sigaction = handle_sigwinch as *const () as libc::sighandler_t;
    sa.sa_flags = 0;
    let mut old_sa: libc::sigaction = unsafe { std::mem::zeroed() };
    unsafe { libc::sigaction(libc::SIGWINCH, &sa, &mut old_sa) };

    terminal::enable_raw_mode()?;
    // Override crossterm's VMIN=1/VTIME=0 with VMIN=0/VTIME=1 (100ms read timeout)
    set_read_timeout(tty_fd, 1);

    execute!(tty_w, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;

    let mut entries = entries;
    let result = run_loop(&mut tty_r, &mut tty_w, tty_fd, &mut entries, &mut stats_data, label_map, selection_counts);

    execute!(tty_w, cursor::Show, DisableMouseCapture, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    // Restore original SIGWINCH handler
    unsafe { libc::sigaction(libc::SIGWINCH, &old_sa, std::ptr::null_mut()) };

    result
}

fn run_loop(
    tty_r: &mut File,
    tty_w: &mut TtyOut,
    tty_fd: i32,
    entries: &mut Vec<HistoryEntry>,
    stats_data: &mut Option<&mut Stats>,
    label_map: &HashMap<String, Label>,
    selection_counts: &HashMap<String, u32>,
) -> io::Result<Option<String>> {
    let mut query = String::new();
    let mut cursor_pos: usize = 0;
    let mut selected: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut multi_select: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut show_help = false;

    let mut need_redraw = true;

    loop {
        let (cols, rows) = tty_size(tty_fd);
        let max_items = (rows as usize).saturating_sub(5);
        let results = history::search_adaptive(entries, &query);

        if selected >= results.len() && !results.is_empty() {
            selected = results.len() - 1;
        }
        if results.is_empty() {
            selected = 0;
            scroll_offset = 0;
        }

        if selected < scroll_offset {
            scroll_offset = selected;
        }
        if selected >= scroll_offset + max_items && max_items > 0 {
            scroll_offset = selected - max_items + 1;
        }

        if need_redraw {
            let mut buf: Vec<u8> = Vec::with_capacity(8192);
            render_frame(&mut buf, &query, cursor_pos, &results, selected, scroll_offset, cols, rows, entries.len(), &multi_select, selection_counts)?;
            if show_help {
                render_help(&mut buf, cols, rows)?;
            }
            tty_w.write_all(&buf)?;
            tty_w.flush()?;
            need_redraw = false;
        }

        // Read key (returns None on VTIME timeout)
        let key = match try_read_key(tty_r)? {
            Some(k) => k,
            None => {
                // Only redraw on resize; otherwise just wait for input
                if RESIZED.swap(false, Ordering::Relaxed) {
                    need_redraw = true;
                }
                continue;
            }
        };

        need_redraw = true;

        if show_help {
            show_help = false;
            continue;
        }

        let char_count = query.chars().count();

        match key {
            Key::CtrlC | Key::CtrlQ | Key::Escape => {
                if !multi_select.is_empty() {
                    multi_select.clear();
                } else {
                    return Ok(None);
                }
            }
            Key::Enter => {
                if let Some(r) = results.get(selected) {
                    let cmd = r.entry.command.clone();
                    if let Some(sd) = stats_data {
                        crate::stats::record_selection(sd, &cmd);
                    }
                    return Ok(Some(cmd));
                }
                return Ok(None);
            }
            Key::Tab => {
                if let Some(result) = results.get(selected) {
                    let cmd = result.entry.command.clone();
                    if multi_select.contains(&cmd) {
                        multi_select.remove(&cmd);
                    } else {
                        multi_select.insert(cmd);
                    }
                    if selected + 1 < results.len() { selected += 1; }
                }
            }
            Key::ShiftTab => {
                if let Some(result) = results.get(selected) {
                    multi_select.remove(&result.entry.command);
                }
                selected = selected.saturating_sub(1);
            }
            Key::Backspace => {
                if cursor_pos > 0 {
                    let byte_idx = query.char_indices().nth(cursor_pos - 1).map(|(i, _)| i);
                    if let Some(idx) = byte_idx {
                        let end_idx = query.char_indices().nth(cursor_pos).map(|(i, _)| i).unwrap_or(query.len());
                        query.replace_range(idx..end_idx, "");
                        cursor_pos -= 1;
                    }
                    selected = 0;
                    scroll_offset = 0;
                }
            }
            Key::Left => {
                cursor_pos = cursor_pos.saturating_sub(1);
            }
            Key::Right => {
                if cursor_pos < char_count { cursor_pos += 1; }
            }
            Key::CtrlA => { cursor_pos = 0; }
            Key::CtrlE => { cursor_pos = char_count; }
            Key::CtrlL => {
                if cursor_pos < char_count {
                    let byte_idx = query.char_indices().nth(cursor_pos).map(|(i, _)| i).unwrap_or(query.len());
                    query.truncate(byte_idx);
                    selected = 0;
                    scroll_offset = 0;
                }
            }
            Key::CtrlU => {
                let half = max_items / 2;
                selected = selected.saturating_sub(half);
            }
            Key::CtrlD => {
                let half = max_items / 2;
                if !results.is_empty() {
                    selected = (selected + half).min(results.len() - 1);
                }
            }
            Key::Up | Key::CtrlP | Key::CtrlK => {
                selected = selected.saturating_sub(1);
            }
            Key::Down | Key::CtrlN | Key::CtrlJ => {
                if selected + 1 < results.len() { selected += 1; }
            }
            Key::CtrlG => {
                // Collect all stale-labeled commands (including hidden freq==1)
                let targets: Vec<String> = entries
                    .iter()
                    .filter(|e| label_map.get(&e.command).is_some_and(|l| l.is_stale()))
                    .map(|e| e.command.clone())
                    .collect();

                if targets.is_empty() {
                    let prompt = " No stale entries found ";
                    show_prompt_bar(tty_w, tty_fd, prompt)?;
                    set_read_blocking(tty_fd);
                    let _ = input::read_key(tty_r)?;
                    set_read_timeout(tty_fd, 1);
                } else {
                    let count = targets.len();
                    let prompt = format!(" Delete {} stale entries? (y/N): ", count);
                    show_prompt_bar(tty_w, tty_fd, &prompt)?;

                    set_read_blocking(tty_fd);
                    let confirmed = loop {
                        match input::read_key(tty_r)? {
                            Key::Char('y') => break true,
                            Key::Char('n') | Key::Char('N') | Key::Escape | Key::CtrlC => break false,
                            _ => continue,
                        }
                    };
                    set_read_timeout(tty_fd, 1);

                    if confirmed {
                        for cmd in &targets {
                            history::delete_command(entries, cmd);
                        }
                        selected = 0;
                        scroll_offset = 0;
                    }
                }
            }
            Key::CtrlX => {
                let results = history::search_adaptive(entries, &query);
                if !multi_select.is_empty() {
                    // Multi-delete: collect commands to delete
                    let targets: Vec<String> = multi_select.iter().cloned().collect();
                    let count = targets.len();
                    let prompt = format!(" Delete {} entries? (y/N/C-c cancel): ", count);
                    show_prompt_bar(tty_w, tty_fd, &prompt)?;

                    // Confirmation loop: y confirms, Ctrl-C clears, others ignored
                    set_read_blocking(tty_fd);
                    let action = loop {
                        match input::read_key(tty_r)? {
                            Key::Char('y') => break true,
                            Key::Char('n') | Key::Char('N') | Key::Escape => break false,
                            Key::CtrlC => {
                                multi_select.clear();
                                break false;
                            }
                            _ => continue,
                        }
                    };
                    set_read_timeout(tty_fd, 1);

                    if action {
                        for cmd in &targets {
                            history::delete_command(entries, cmd);
                        }
                        multi_select.clear();
                        selected = 0;
                        scroll_offset = 0;
                    }
                } else if let Some(result) = results.get(selected) {
                    // Single delete
                    let cmd = result.entry.command.clone();
                    let prompt = format!(" Delete? (y/N): {}", truncate_str(&cmd, cols as usize - 20));
                    show_prompt_bar(tty_w, tty_fd, &prompt)?;

                    set_read_blocking(tty_fd);
                    let confirmed = matches!(input::read_key(tty_r)?, Key::Char('y'));
                    set_read_timeout(tty_fd, 1);

                    if confirmed {
                        history::delete_command(entries, &cmd);
                        if selected > 0 && selected >= entries.len() {
                            selected = selected.saturating_sub(1);
                        }
                    }
                }
            }
            Key::CtrlSlash | Key::Char('?') => {
                show_help = true;
            }
            Key::Char(c) => {
                let byte_idx = query.char_indices().nth(cursor_pos).map(|(i, _)| i).unwrap_or(query.len());
                query.insert(byte_idx, c);
                cursor_pos += 1;
                selected = 0;
                scroll_offset = 0;
            }
            Key::Unknown => {}
        }
    }
}

fn show_prompt_bar(tty_w: &mut TtyOut, tty_fd: i32, prompt: &str) -> io::Result<()> {
    let (cols, rows) = tty_size(tty_fd);
    let mut cbuf: Vec<u8> = Vec::with_capacity(1024);
    let prompt_row = rows.saturating_sub(1);
    queue!(cbuf, cursor::MoveTo(0, prompt_row))?;
    queue!(cbuf, SetBackgroundColor(COLOR_WARN_BG), SetForegroundColor(COLOR_WARN_FG))?;
    let display = truncate_str(prompt, cols as usize);
    write!(cbuf, "{display}")?;
    let pad = (cols as usize).saturating_sub(str_width(&display));
    for _ in 0..pad { write!(cbuf, " ")?; }
    queue!(cbuf, SetAttribute(Attribute::Reset))?;
    tty_w.write_all(&cbuf)?;
    tty_w.flush()?;
    Ok(())
}

// ── Help overlay ────────────────────────────────────────────────────────

const HELP_SHORTCUTS: &[(&str, &str)] = &[
    ("↑  C-p  C-k",    "Selection up"),
    ("↓  C-n  C-j",    "Selection down"),
    ("C-u",            "Half page up"),
    ("C-d",            "Half page down"),
    ("Enter",          "Select command"),
    ("Tab",            "Multi-select toggle"),
    ("Shift-Tab",      "Deselect + move up"),
    ("C-g",            "Select all stale"),
    ("C-x",            "Delete selected"),
    ("Esc  C-c  C-q",  "Quit / clear select"),
    ("← / →",          "Cursor move"),
    ("C-a / C-e",      "Cursor home/end"),
    ("C-l",            "Delete to end"),
    ("?  C-/",         "This help"),
];

const COLOR_HELP_BG: Color = Color::Rgb { r: 35, g: 35, b: 40 };

fn render_help(buf: &mut Vec<u8>, cols: u16, rows: u16) -> io::Result<()> {
    // ── Layout constants ──
    let pad: usize = 3;
    let key_w: usize = 16;
    let gap: usize = 3;
    let desc_w: usize = 19;
    let inner: usize = pad + key_w + gap + desc_w + pad;

    let box_w = (inner + 2) as u16; // outer borders
    let content_rows = HELP_SHORTCUTS.len();
    let box_h = content_rows as u16 + 6; // +2 top padding, +2 bottom (hint + blank), +2 border
    let x = cols.saturating_sub(box_w) / 2;
    let y = rows.saturating_sub(box_h) / 2;

    // ── Top border ──
    let title = " Shortcuts ";
    queue!(buf, cursor::MoveTo(x, y), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "╭")?;
    queue!(buf, SetForegroundColor(COLOR_TITLE))?;
    write!(buf, "{title}")?;
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    let fill = inner.saturating_sub(title.len());
    for _ in 0..fill { write!(buf, "─")?; }
    write!(buf, "╮")?;

    // ── Top padding ──
    for r in 1..=2u16 {
        queue!(buf, cursor::MoveTo(x, y + r), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
        write!(buf, "│")?;
        for _ in 0..inner { write!(buf, " ")?; }
        write!(buf, "│")?;
    }

    // ── Content rows ──
    for i in 0..content_rows {
        let row = y + 3 + i as u16;
        queue!(buf, cursor::MoveTo(x, row), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
        write!(buf, "│")?;

        if let Some((key, desc)) = HELP_SHORTCUTS.get(i) {
            queue!(buf, SetForegroundColor(COLOR_MATCH))?;
            write!(buf, "{:>p$}{key:<kw$}", "", p = pad, kw = key_w)?;
            queue!(buf, SetForegroundColor(COLOR_DIM))?;
            for _ in 0..gap { write!(buf, " ")?; }
            queue!(buf, SetForegroundColor(COLOR_TEXT))?;
            write!(buf, "{desc:<dw$}", dw = desc_w)?;
            for _ in 0..pad { write!(buf, " ")?; }
        } else {
            for _ in 0..inner { write!(buf, " ")?; }
        }

        queue!(buf, SetForegroundColor(COLOR_BORDER))?;
        write!(buf, "│")?;
    }

    // ── Blank row ──
    let blank_row = y + 3 + content_rows as u16;
    queue!(buf, cursor::MoveTo(x, blank_row), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "│")?;
    for _ in 0..inner { write!(buf, " ")?; }
    write!(buf, "│")?;

    // ── Hint row ──
    let hint_row = blank_row + 1;
    let hint = "Press any key to close";
    let hint_w = hint.len();
    let pad_hint_l = inner.saturating_sub(hint_w) / 2;
    let pad_hint_r = inner.saturating_sub(hint_w).saturating_sub(pad_hint_l);
    queue!(buf, cursor::MoveTo(x, hint_row), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "│")?;
    queue!(buf, SetForegroundColor(COLOR_DIM))?;
    for _ in 0..pad_hint_l { write!(buf, " ")?; }
    write!(buf, "{hint}")?;
    for _ in 0..pad_hint_r { write!(buf, " ")?; }
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "│")?;

    // ── Bottom border ──
    let bot = hint_row + 1;
    queue!(buf, cursor::MoveTo(x, bot), SetBackgroundColor(COLOR_HELP_BG), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "╰")?;
    for _ in 0..inner { write!(buf, "─")?; }
    write!(buf, "╯")?;

    queue!(buf, SetAttribute(Attribute::Reset))?;
    Ok(())
}

// ── Rendering ───────────────────────────────────────────────────────────

fn write_hline(buf: &mut Vec<u8>, count: usize) -> io::Result<()> {
    for _ in 0..count { write!(buf, "─")?; }
    Ok(())
}

/// Position cursor and write the right-side border character
fn right_border(buf: &mut Vec<u8>, col: u16, row: u16, ch: &str) -> io::Result<()> {
    queue!(buf, cursor::MoveTo(col, row))?;
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "{ch}")?;
    queue!(buf, SetForegroundColor(Color::Reset))?;
    Ok(())
}

fn render_frame(
    buf: &mut Vec<u8>,
    query: &str,
    cursor_pos: usize,
    results: &[SearchResult],
    selected: usize,
    scroll_offset: usize,
    cols: u16,
    rows: u16,
    total_count: usize,
    multi_select: &std::collections::HashSet<String>,
    selection_counts: &HashMap<String, u32>,
) -> io::Result<()> {
    let max_items = (rows as usize).saturating_sub(5);
    let w = cols as usize;
    let rc = cols.saturating_sub(1); // right-border column

    queue!(buf, SetAttribute(Attribute::Reset), cursor::MoveTo(0, 0), terminal::Clear(ClearType::CurrentLine))?;

    // Row 0: top border with title
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "╭")?;
    queue!(buf, SetForegroundColor(COLOR_TITLE))?;
    write!(buf, " bh ")?;
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    let title_w = 1 + 4; // "╭" + " bh "
    write_hline(buf, w.saturating_sub(title_w + 1))?;
    right_border(buf, rc, 0, "╮")?;

    // Row 1: input line
    queue!(buf, cursor::MoveTo(0, 1), terminal::Clear(ClearType::CurrentLine), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "│")?;
    queue!(buf, SetForegroundColor(COLOR_TEXT))?;
    write!(buf, " > ")?;

    // Render query with cursor
    let query_chars: Vec<char> = query.chars().collect();
    let max_query_w = w.saturating_sub(6); // border + " > " + border + padding
    let mut qw: usize = 0;
    for (i, &ch) in query_chars.iter().enumerate() {
        let cw = char_width(ch);
        if qw + cw > max_query_w { break; }
        if i == cursor_pos {
            queue!(buf, SetBackgroundColor(COLOR_CURSOR), SetForegroundColor(Color::Black))?;
            write!(buf, "{ch}")?;
            queue!(buf, SetAttribute(Attribute::Reset))?;
        } else {
            queue!(buf, SetForegroundColor(COLOR_TEXT))?;
            write!(buf, "{ch}")?;
        }
        qw += cw;
    }
    // If cursor is at the end, show block cursor
    if cursor_pos >= query_chars.len() {
        queue!(buf, SetBackgroundColor(COLOR_CURSOR), SetForegroundColor(Color::Black))?;
        write!(buf, " ")?;
        queue!(buf, SetAttribute(Attribute::Reset))?;
    }

    right_border(buf, rc, 1, "│")?;

    // Row 2: separator with count on the right
    queue!(buf, cursor::MoveTo(0, 2), terminal::Clear(ClearType::CurrentLine), SetForegroundColor(COLOR_BORDER))?;
    let count_display = format!(" {}/{} ", results.len(), total_count);
    let count_w = str_width(&count_display);
    let fill = w.saturating_sub(1 + count_w + 1);
    write!(buf, "│")?;
    write_hline(buf, fill)?;
    queue!(buf, SetForegroundColor(COLOR_DIM))?;
    write!(buf, "{count_display}")?;
    queue!(buf, SetForegroundColor(COLOR_BORDER))?;
    right_border(buf, rc, 2, "│")?;

    // Rows 3+: results
    let content_w = w.saturating_sub(2);
    for i in 0..max_items {
        let result_idx = scroll_offset + i;
        let row = 3 + i as u16;
        if row >= rows.saturating_sub(1) { break; }

        queue!(buf, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine), SetForegroundColor(COLOR_BORDER))?;
        write!(buf, "│")?;

        if let Some(result) = results.get(result_idx) {
            let is_multi = multi_select.contains(&result.entry.command);
            if result_idx == selected {
                queue!(buf, SetBackgroundColor(COLOR_SELECTED_BG))?;
            }
            if is_multi {
                queue!(buf, SetForegroundColor(COLOR_MULTI))?;
                write!(buf, " * ")?;
            } else if result_idx == selected {
                queue!(buf, SetForegroundColor(COLOR_TEXT))?;
                write!(buf, " > ")?;
            } else {
                queue!(buf, SetForegroundColor(COLOR_TEXT))?;
                write!(buf, "   ")?;
            }
            let sel_count = selection_counts.get(&result.entry.command).copied().unwrap_or(0);
            let count_display = if sel_count > 0 { format!("{sel_count}") } else { String::new() };
            let count_width = if sel_count > 0 { str_width(&count_display) + 1 } else { 0 }; // +1 for leading space
            let entry_width = content_w.saturating_sub(3).saturating_sub(count_width);
            let written = render_entry(buf, result, entry_width, result_idx == selected)?;

            // Pad remaining space and render count at right edge
            let pad = entry_width.saturating_sub(written);
            for _ in 0..pad { write!(buf, " ")?; }

            if sel_count > 0 {
                queue!(buf, SetForegroundColor(COLOR_DIM))?;
                write!(buf, " {count_display}")?;
            }
            queue!(buf, SetAttribute(Attribute::Reset))?;
        }

        right_border(buf, rc, row, "│")?;
    }

    // Bottom border
    let bottom = rows.saturating_sub(2).min(3 + max_items as u16);
    queue!(buf, cursor::MoveTo(0, bottom), terminal::Clear(ClearType::CurrentLine), SetForegroundColor(COLOR_BORDER))?;
    write!(buf, "╰")?;
    write_hline(buf, w.saturating_sub(2))?;
    right_border(buf, rc, bottom, "╯")?;

    // Key hints below the border
    let hint_row = bottom + 1;
    if hint_row < rows {
        queue!(buf, cursor::MoveTo(0, hint_row), terminal::Clear(ClearType::CurrentLine))?;
        let hint = "?: help";
        let hint_w = str_width(hint);
        let x = (cols as usize).saturating_sub(hint_w + 1);
        queue!(buf, cursor::MoveTo(x as u16, hint_row), SetForegroundColor(COLOR_DIM))?;
        write!(buf, "{hint}")?;
    }

    // Clear any stale rows below the hints
    for r in (hint_row + 1)..rows {
        queue!(buf, cursor::MoveTo(0, r), terminal::Clear(ClearType::CurrentLine))?;
    }

    queue!(buf, SetAttribute(Attribute::Reset))?;

    Ok(())
}

fn render_entry(buf: &mut Vec<u8>, result: &SearchResult, available: usize, is_selected: bool) -> io::Result<usize> {
    let cmd = &result.entry.command;
    let cmd_max = available;
    let mut written: usize = 0;
    let mut in_hl = false;

    for (bi, ch) in cmd.char_indices() {
        let cw = char_width(ch);
        if cw == 0 { continue; }
        if written + cw > cmd_max {
            if in_hl {
                queue!(buf, SetForegroundColor(COLOR_TEXT))?;
                if is_selected {
                    queue!(buf, SetBackgroundColor(COLOR_SELECTED_BG))?;
                }
            }
            write!(buf, "…")?;
            written += 1;
            break;
        }

        let is_match = result.match_positions.contains(&bi);
        if is_match && !in_hl {
            queue!(buf, SetForegroundColor(COLOR_MATCH))?;
            in_hl = true;
        } else if !is_match && in_hl {
            queue!(buf, SetForegroundColor(COLOR_TEXT))?;
            in_hl = false;
        }

        write!(buf, "{ch}")?;
        written += cw;
    }

    if in_hl {
        queue!(buf, SetForegroundColor(COLOR_TEXT))?;
    }

    Ok(written)
}
