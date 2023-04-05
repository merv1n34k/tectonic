//! A module for using a texpresso connection as bundle [`DirBundle`].
//!
use std::{
    fs,
    path::{Path, PathBuf}, convert::Infallible,
};
use tectonic_errors::prelude::*;
use tectonic_io_base::{InputHandle, IoProvider, OpenResult};
use tectonic_status_base::StatusBackend;
use texpresso_protocol::Client;

use super::Bundle;

/// A "bundle" of a bunch of files in a directory.
///
/// This implementation essentially just wraps
/// [`tectonic_io_base::filesystem::FilesystemIo`], ensuring that it is
/// read-only, self-contained, and implements the [`Bundle`] trait. The
/// directory should contain a file named `SHA256SUM` if the bundle fingerprint
/// will be needed.
pub struct TexpressoBundle {
    client : Client,
    fallback: Box<dyn Bundle>,
}

impl TexpressoBundle {
    /// Create a new directory bundle.
    ///
    /// No validation of the input path is performed, which is why this function
    /// is infallible.
    pub fn new(client: Client, fallback: Box<dyn Bundle>) -> TexpressoBundle {
        TexpressoBundle{client, fallback}
    }
}

impl IoProvider for TexpressoBundle {
    /// Open the named file for output.
    fn output_open_name(&mut self, _name: &str) -> OpenResult<OutputHandle> {
        OpenResult::NotAvailable
    }

    /// Open the standard output stream.
    fn output_open_stdout(&mut self) -> OpenResult<OutputHandle> {
        OpenResult::NotAvailable
    }

    fn input_open_name(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        // self.client.open(file, path, mode)
        // self.0.input_open_name(name, status)
        panic!("TODO")
    }

    fn input_open_format(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        self.fallback.input_open_name(name, status)
    }
}

impl Bundle for TexpressoBundle {
    fn all_files(&mut self, status: &mut dyn StatusBackend) -> Result<Vec<String>> {
        self.fallback.all_files(status)
    }
}
