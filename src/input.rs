use std::fs::File;
use std::io::{self, Read};

#[derive(Debug)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    ShiftTab,
    Up,
    Down,
    Left,
    Right,
    Escape,
    CtrlA,
    CtrlC,
    CtrlD,
    CtrlE,
    CtrlJ,
    CtrlK,
    CtrlL,
    CtrlN,
    CtrlP,
    CtrlQ,
    CtrlU,
    CtrlX,
    CtrlSlash,
    Unknown,
}

pub fn read_key(tty: &mut File) -> io::Result<Key> {
    let mut buf = [0u8; 1];
    tty.read_exact(&mut buf)?;

    match buf[0] {
        0x01 => Ok(Key::CtrlA),
        0x03 => Ok(Key::CtrlC),
        0x09 => Ok(Key::Tab),
        0x04 => Ok(Key::CtrlD),
        0x05 => Ok(Key::CtrlE),
        0x0a => Ok(Key::CtrlJ),
        0x0b => Ok(Key::CtrlK),
        0x0c => Ok(Key::CtrlL),
        0x0d => Ok(Key::Enter),
        0x0e => Ok(Key::CtrlN),
        0x10 => Ok(Key::CtrlP),
        0x11 => Ok(Key::CtrlQ),
        0x15 => Ok(Key::CtrlU),
        0x18 => Ok(Key::CtrlX),
        0x1f => Ok(Key::CtrlSlash),
        0x7f | 0x08 => Ok(Key::Backspace),
        0x1b => read_escape_seq(tty),
        b if (0x20..0x7f).contains(&b) => Ok(Key::Char(b as char)),
        b if b >= 0xc0 => read_utf8_char(tty, b),
        _ => Ok(Key::Unknown),
    }
}

fn read_escape_seq(tty: &mut File) -> io::Result<Key> {
    let mut seq = [0u8; 2];
    match tty.read(&mut seq) {
        Ok(2) => match seq {
            [b'[', b'A'] => Ok(Key::Up),
            [b'[', b'B'] => Ok(Key::Down),
            [b'[', b'C'] => Ok(Key::Right),
            [b'[', b'D'] => Ok(Key::Left),
            [b'[', b'Z'] => Ok(Key::ShiftTab),
            // Legacy mouse: \x1b[M + 3 bytes (button, x, y)
            [b'[', b'M'] => {
                let mut discard = [0u8; 3];
                let _ = tty.read(&mut discard);
                Ok(Key::Unknown)
            }
            // SGR mouse: \x1b[< + digits/semicolons + M/m
            [b'[', b'<'] => {
                drain_until_alpha(tty);
                Ok(Key::Unknown)
            }
            [b'[', _] => Ok(Key::Unknown),
            _ => Ok(Key::Escape),
        },
        _ => Ok(Key::Escape),
    }
}

fn drain_until_alpha(tty: &mut File) {
    let mut b = [0u8; 1];
    loop {
        match tty.read(&mut b) {
            Ok(1) if b[0].is_ascii_alphabetic() => break,
            Ok(1) => continue,
            _ => break,
        }
    }
}

fn read_utf8_char(tty: &mut File, first: u8) -> io::Result<Key> {
    let len = if first < 0xe0 { 2 } else if first < 0xf0 { 3 } else { 4 };
    let mut bytes = vec![first];
    let mut rest = vec![0u8; len - 1];
    tty.read_exact(&mut rest)?;
    bytes.extend_from_slice(&rest);
    match std::str::from_utf8(&bytes) {
        Ok(s) => Ok(s.chars().next().map_or(Key::Unknown, Key::Char)),
        Err(_) => Ok(Key::Unknown),
    }
}
