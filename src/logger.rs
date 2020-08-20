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
    io::{self, Write},
    sync::RwLock,
};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// A simple logger.
pub struct Logger {
    inner: RwLock<InnerLogger>,
    trace_cspec: ColorSpec,
    debug_cspec: ColorSpec,
    info_cspec: ColorSpec,
    warn_cspec: ColorSpec,
    error_cspec: ColorSpec,
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

        warn_cspec.set_fg(Some(Color::Yellow)).set_bold(true);
        info_cspec.set_fg(Some(Color::Green)).set_bold(true);
        error_cspec.set_fg(Some(Color::Red)).set_bold(true);

        Logger {
            inner: RwLock::new(InnerLogger { stdout, stderr }),
            trace_cspec,
            debug_cspec,
            info_cspec,
            warn_cspec,
            error_cspec,
        }
    };
}

impl Logger {
    /// Set up this type as the global static logger.
    pub fn init() -> Result<(), log::SetLoggerError> {
        log::set_logger(&*LOGGER)
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
        }
    }

    fn flush(&self) {
        let _r = io::stdout().flush();
    }
}
