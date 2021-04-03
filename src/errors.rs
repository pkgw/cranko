// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Error handling for Cranko.

use log::error;
use thiserror::Error as ThisError;

/// The generic error type, for complex operations that can fail for a wide
/// range of reasons. This type is a reexport of the `anyhow` 1.x series Error
/// type. There is an appeal to not explicitly committing ourselves to using
/// this particular error implementation, but the `anyhow` error type has a
/// sufficient number of special methods and traits that it would be pretty
/// tedious to re-implement them all while pretending that we're using some
/// different type.
pub use anyhow::Error;

/// A preloaded result type, in which the error type is our generic error type.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Default, ThisError)]
#[error("{message}")]
pub struct AnnotatedReport {
    // The main contextual message
    message: String,

    // Additional annotations that can be displayed after the primary error
    // report.
    notes: Vec<String>,
}

impl AnnotatedReport {
    pub fn set_message(&mut self, m: String) {
        self.message = m;
    }

    pub fn add_note(&mut self, n: String) {
        self.notes.push(n);
    }

    pub fn notes(&self) -> &[String] {
        &self.notes[..]
    }
}

// Get this name for atry!
#[doc(hidden)]
pub use anyhow::Context;

/// "Annotated try” — like `try!`, but with the ability to add extended context
/// to the error message. This tries to provide a bit more syntactic sugar than
/// anyhow's `with_context()`, and it supports our AnnotatedReport context type.
#[macro_export]
macro_rules! atry {
    (@aa $ar:ident [ $($inner:tt)+ ] ) => {
        $ar.set_message(format!($($inner)+));
    };

    (@aa $ar:ident ( note $($inner:tt)+ ) ) => {
        $ar.add_note(format!($($inner)+));
    };

    ($op:expr ; $( $annotation:tt )+) => {{
        use $crate::errors::Context;
        $op.with_context(|| {
            let mut ar = $crate::errors::AnnotatedReport::default();
            $(
                atry!(@aa ar $annotation);
            )+
            ar
        })?
    }};
}

/// "annotated ok_or” — like `Option::ok_or_else()?`, but with the ability to add
/// extended context to the error
#[macro_export]
macro_rules! a_ok_or {
    (@aa $ar:ident [ $($inner:tt)+ ] ) => {
        $ar.set_message(format!($($inner)+));
    };

    (@aa $ar:ident ( note $($inner:tt)+ ) ) => {
        $ar.add_note(format!($($inner)+));
    };

    ($option:expr ; $( $annotation:tt )+) => {{
        use $crate::atry;
        $option.ok_or_else(|| {
            let mut ar = $crate::errors::AnnotatedReport::default();
            $(
                atry!(@aa ar $annotation);
            )+
            ar
        })?
    }};
}

pub fn report(r: Result<i32>) -> i32 {
    let err = match r {
        Ok(c) => return c,
        Err(e) => e,
    };

    let mut notes = Vec::new();

    eprintln!();
    error!("{}", err);

    if let Some(ann) = err.downcast_ref::<AnnotatedReport>() {
        notes.extend(ann.notes());
    }

    err.chain().skip(1).for_each(|cause| {
        crate::logger::Logger::print_cause(cause);

        if let Some(ann) = cause.downcast_ref::<AnnotatedReport>() {
            notes.extend(ann.notes());
        }
    });

    for note in &notes {
        eprintln!();
        crate::logger::Logger::print_err_note(note);
    }

    1
}
