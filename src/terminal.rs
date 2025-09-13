use anyhow::Result;
use std::any::Any;
use std::io::{self, IsTerminal};

#[cfg(unix)]
use libc::{STDOUT_FILENO, ioctl};

#[cfg(windows)]
use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
#[cfg(windows)]
use winapi::um::processenv::GetStdHandle;
#[cfg(windows)]
use winapi::um::winbase::STD_OUTPUT_HANDLE;
#[cfg(windows)]
use winapi::um::wincon::{
    ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING,
};

pub fn is_terminal() -> bool {
    io::stdout().is_terminal()
}

pub fn get_terminal_size() -> Result<(usize, usize)> {
    #[cfg(unix)]
    {
        get_terminal_size_unix()
    }

    #[cfg(windows)]
    {
        get_terminal_size_windows()
    }
}

#[cfg(unix)]
fn get_terminal_size_unix() -> Result<(usize, usize)> {
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        let result = ioctl(STDOUT_FILENO, libc::TIOCGWINSZ, &mut size as *mut _);

        if result == 0 {
            Ok((size.ws_col as usize, size.ws_row as usize))
        } else {
            Ok((80, 24)) // Default fallback
        }
    }
}

#[cfg(windows)]
fn get_terminal_size_windows() -> Result<(usize, usize)> {
    // For Windows, we'll use a simple approach
    // In a real implementation, you'd use GetConsoleScreenBufferInfo
    Ok((80, 24)) // Default fallback
}

pub struct TermState(Box<dyn Any>);

impl Drop for TermState {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if let Ok(termios) =
                <Box<dyn std::any::Any> as Clone>::clone(&self.0).downcast::<libc::termios>()
            {
                enable_echo_unix(*termios);
            }
        }

        #[cfg(windows)]
        {
            if let Ok(mode) = self.0.downcast::<u32>() {
                enable_echo_windows(*mode);
            }
        }
    }
}

pub fn disable_echo() -> TermState {
    #[cfg(unix)]
    {
        TermState(disable_echo_unix())
    }

    #[cfg(windows)]
    {
        TermState(disable_echo_windows())
    }
}

#[cfg(unix)]
fn disable_echo_unix() -> Box<dyn Any> {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        let result = libc::tcgetattr(STDOUT_FILENO, &mut termios);

        if result == 0 {
            let mut new_termios = termios;
            new_termios.c_lflag &= !libc::ECHO;
            new_termios.c_lflag |= libc::ICANON | libc::ISIG;
            new_termios.c_iflag |= libc::ICRNL;

            let _ = libc::tcsetattr(STDOUT_FILENO, libc::TCSANOW, &new_termios);
            Box::new(termios)
        } else {
            Box::new(termios)
        }
    }
}

#[cfg(unix)]
fn enable_echo_unix(termios: libc::termios) {
    unsafe {
        let _ = libc::tcsetattr(STDOUT_FILENO, libc::TCSANOW, &termios);
    }
}

#[cfg(windows)]
fn disable_echo_windows() -> Box<dyn Any> {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode: u32 = 0;

        if GetConsoleMode(handle, &mut mode) != 0 {
            let new_mode = mode & !ENABLE_ECHO_INPUT;
            let new_mode = new_mode
                | ENABLE_PROCESSED_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_VIRTUAL_TERMINAL_PROCESSING;

            let _ = SetConsoleMode(handle, new_mode);
            Box::new(mode)
        } else {
            Box::new(mode)
        }
    }
}

#[cfg(windows)]
fn enable_echo_windows(mode: u32) {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let _ = SetConsoleMode(handle, mode);
    }
}
