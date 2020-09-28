// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! A very simple logger.
//!
//! I've had a shockingly difficult time finding a simple logging option the
//! provides the CLI look that I want.
//!
//! Loosely derived from the logger in ripgrep, but with a few more bells and
//! whistles.

use lazy_static::lazy_static;
use log::{Level, Log};
use std::{
    fmt::Display,
    io::{self, Write},
    sync::RwLock,
};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

fn get_wrap_width() -> usize {
    use terminal_size::{terminal_size, Height, Width};

    if let Some((Width(w), Height(_))) = terminal_size() {
        if w > 80 {
            80
        } else if w < 20 {
            20
        } else {
            w as usize
        }
    } else {
        80
    }
}

/// A simple logger.
pub struct Logger {
    inner: RwLock<InnerLogger>,
    trace_cspec: ColorSpec,
    debug_cspec: ColorSpec,
    info_cspec: ColorSpec,
    warn_cspec: ColorSpec,
    error_cspec: ColorSpec,
    highlight_cspec: ColorSpec,
}

pub struct InnerLogger {
    stdout: StandardStream,
    stderr: StandardStream,
}

lazy_static! {
    static ref LOGGER: Logger = {
        let stdout = StandardStream::stdout(ColorChoice::Auto);
        let stderr = StandardStream::stderr(ColorChoice::Auto);
        let trace_cspec = ColorSpec::new();
        let debug_cspec = ColorSpec::new();
        let mut info_cspec = ColorSpec::new();
        let mut warn_cspec = ColorSpec::new();
        let mut error_cspec = ColorSpec::new();
        let mut highlight_cspec = ColorSpec::new();

        warn_cspec.set_fg(Some(Color::Yellow)).set_bold(true);
        info_cspec.set_fg(Some(Color::Green)).set_bold(true);
        error_cspec.set_fg(Some(Color::Red)).set_bold(true);
        highlight_cspec.set_fg(Some(Color::Cyan)).set_bold(true);

        Logger {
            inner: RwLock::new(InnerLogger { stdout, stderr }),
            trace_cspec,
            debug_cspec,
            info_cspec,
            warn_cspec,
            error_cspec,
            highlight_cspec,
        }
    };
}

impl Logger {
    /// Set up this type as the global static logger.
    pub fn init() -> Result<(), log::SetLoggerError> {
        log::set_logger(&*LOGGER)
    }

    pub fn print_cause(err: &(dyn std::error::Error + 'static)) {
        if let Ok(mut inner) = LOGGER.inner.write() {
            let _r = inner.stderr.set_color(&LOGGER.error_cspec);
            let _r = write!(&mut inner.stderr, "caused by:");
            let _r = inner.stderr.reset();
            let _r = writeln!(&mut inner.stderr, " {}", err);
        } else {
            eprintln!("caused by: {}", err);
        }
    }

    pub fn print_err_note<T: Display>(msg: T) {
        let msg = msg.to_string();
        let mut first = true;

        for line in textwrap::wrap_iter(&msg, get_wrap_width() - 6) {
            if first {
                first = false;

                if let Ok(mut inner) = LOGGER.inner.write() {
                    let _r = inner.stderr.set_color(&LOGGER.error_cspec);
                    let _r = write!(&mut inner.stderr, "note:");
                    let _r = inner.stderr.reset();
                    let _r = writeln!(&mut inner.stderr, " {}", line);
                } else {
                    eprintln!("note: {}", line);
                }
            } else {
                eprintln!("      {}", line);
            }
        }
    }

    pub fn println_highlighted<T1: Display, T2: Display, T3: Display>(
        before: T1,
        highlight: T2,
        after: T3,
    ) {
        if let Ok(mut inner) = LOGGER.inner.write() {
            let _r = write!(&mut inner.stdout, "{}", before);
            let _r = inner.stdout.set_color(&LOGGER.highlight_cspec);
            let _r = write!(&mut inner.stdout, "{}", highlight);
            let _r = inner.stdout.reset();
            let _r = writeln!(&mut inner.stdout, "{}", after);
        } else {
            println!("{}{}{}", before, highlight, after);
        }
    }
}

impl Log for Logger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        // Rely on `log::set_max_level()` for filtering
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut inner) = self.inner.write() {
            match record.level() {
                Level::Trace => {
                    let _r = inner.stderr.set_color(&self.trace_cspec);
                    let _r = write!(&mut inner.stderr, "trace:");
                    let _r = inner.stderr.reset();
                    let _r = writeln!(&mut inner.stderr, " {}", record.args());
                }

                Level::Debug => {
                    let _r = inner.stderr.set_color(&self.debug_cspec);
                    let _r = write!(&mut inner.stderr, "debug:");
                    let _r = inner.stderr.reset();
                    let _r = writeln!(&mut inner.stderr, " {}", record.args());
                }

                Level::Info => {
                    let _r = inner.stdout.set_color(&self.info_cspec);
                    let _r = write!(&mut inner.stdout, "info:");
                    let _r = inner.stdout.reset();
                    let _r = writeln!(&mut inner.stdout, " {}", record.args());
                }

                Level::Warn => {
                    let _r = inner.stderr.set_color(&self.warn_cspec);
                    let _r = write!(&mut inner.stderr, "warning:");
                    let _r = inner.stderr.reset();
                    let _r = writeln!(&mut inner.stderr, " {}", record.args());
                }

                Level::Error => {
                    let _r = inner.stderr.set_color(&self.error_cspec);
                    let _r = write!(&mut inner.stderr, "error:");
                    let _r = inner.stderr.reset();
                    let _r = writeln!(&mut inner.stderr, " {}", record.args());
                }
            }
        } else {
            match record.level() {
                Level::Trace => {
                    eprintln!("trace: {}", record.args());
                }

                Level::Debug => {
                    eprintln!("debug: {}", record.args());
                }

                Level::Info => {
                    println!("info: {}", record.args());
                }

                Level::Warn => {
                    eprintln!("warning: {}", record.args());
                }

                Level::Error => {
                    eprintln!("error: {}", record.args());
                }
            }
        }
    }

    fn flush(&self) {
        let _r = io::stdout().flush();
    }
}
