// Copyright 2017-2020 the Tectonic Project
// Licensed under the MIT License.

//! A basic status-reporting backend that prints messages via stdio.

use std::{
    fmt::Arguments,
    io::{self, Write, stderr},
};
use tectonic_errors::Error;
use tectonic_io_base::texpresso::TexpressoStdout;

use super::{ChatterLevel, MessageKind, StatusBackend};

/// A basic status-reporting backend that prints messages via stdio.
#[derive(Clone)]
pub struct TexpressoStatusBackend {
    chatter: ChatterLevel,
    output: TexpressoStdout,
}

impl TexpressoStatusBackend {
    /// Create a new backend with the specified chatter level.
    ///
    /// To use the default chatter level, you can also use [`Self::default`].
    pub fn new(chatter: ChatterLevel, output: TexpressoStdout) -> Self {
        TexpressoStatusBackend {chatter, output}
    }

}

fn write_report<T : Write>(output: &mut T,
                           prefix: &str,
                           args: Arguments,
                           err: Option<&Error>)
{
    writeln!(output, "{prefix} {args}").unwrap();
    if let Some(e) = err {
        for item in e.chain() {
            writeln!(output, "caused by: {item}").unwrap();
        }
    }
}

impl StatusBackend for TexpressoStatusBackend {

    fn report(&mut self, kind: MessageKind, args: Arguments, err: Option<&Error>) {
        if self.chatter.suppress_message(kind) {
            return;
        }
        match kind {
            MessageKind::Note => write_report(&mut stderr(), "note:", args, err),
            MessageKind::Warning => write_report(&mut self.output, "warning:", args, err),
            MessageKind::Error => write_report(&mut self.output, "error:", args, err),
        };
    }

    fn report_error(&mut self, err: &Error) {
        let mut prefix = "error";

        for item in err.chain() {
            writeln!(self.output, "{prefix}: {item}").unwrap();
            prefix = "caused by";
        }
    }

    fn note_highlighted(&mut self, before: &str, highlighted: &str, after: &str) {
        self.report(
            MessageKind::Note,
            format_args!("{before}{highlighted}{after}"),
            None,
        );
    }

    fn dump_error_logs(&mut self, output: &[u8]) {
        writeln!(self.output,
            "==============================================================================="
        ).unwrap();

        io::stderr()
            .write_all(output)
            .expect("write to stderr failed");

        writeln!(self.output,
            "==============================================================================="
        ).unwrap();
    }
}
