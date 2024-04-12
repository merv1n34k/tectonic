// Copyright 2017-2021 the Tectonic Project
// Licensed under the MIT License.

//! A module for using TeXpresso server as a bundle [`TexpressoBundle`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write, Cursor};
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use tectonic_io_base::{InputHandle, InputOrigin, IoProvider, OpenResult};
use tectonic_status_base::StatusBackend;

use super::Bundle;
use tectonic_errors::prelude::Result;

/// A "bundle" of a bunch of files in a directory.
///
/// This implementation essentially just wraps
/// [`tectonic_io_base::filesystem::FilesystemIo`], ensuring that it is
/// read-only, self-contained, and implements the [`Bundle`] trait. The
/// directory should contain a file named `SHA256SUM` if the bundle fingerprint
/// will be needed.
pub struct TexpressoBundle {
    input: File,
    output: File,
    lock: i32,
    cached: HashMap<String, Option<PathBuf>>,
}

impl TexpressoBundle {
    /// Create a new directory bundle.
    ///
    /// No validation of the input path is performed, which is why this function
    /// is infallible.
    pub fn connect(url: &str) -> Option<TexpressoBundle> {
        let suffix =  url.strip_prefix("texpresso-bundle://")?;
        let mut bits = suffix.rsplitn(3, ',');
        let (input, output, lock) =
            match (bits.next(), bits.next(), bits.next(), bits.next()) {
                (Some(s), Some(t), Some(r), None) => (r, t, s),
                _ => return None,
            };
        let input = input.parse::<i32>().ok()?;
        let output = output.parse::<i32>().ok()?;
        let lock = lock.parse::<i32>().ok()?;
        let input = unsafe {File::from_raw_fd(input)};
        let output = unsafe {File::from_raw_fd(output)};
        let cached = HashMap::new();
        Some(TexpressoBundle{input,output,lock,cached})
    }
}

impl Bundle for TexpressoBundle {
    fn all_files(&mut self, _status: &mut dyn StatusBackend) -> Result<Vec<String>> {
        Result::Ok(Vec::new())
    }
}

fn my_flock(fd: i32, op: i32) {
    loop {
        let result = unsafe {libc::flock(fd, op)};
        if result == -1 {
            match std::io::Error::last_os_error().raw_os_error() {
                Some(err) if err == libc::EINTR => continue,
                _ => panic!(),
            };
        };
        break;
    }
}

enum Answer {
    E(String),
    P(PathBuf),
    C(Vec<u8>),
}

fn read_data(input: &mut File) -> Vec<u8> {
    let mut size = [0; 8];
    input.read_exact(&mut size).unwrap();
    let size = u64::from_le_bytes(size);
    let mut data = vec![0u8; size as usize];
    input.read_exact(&mut data).unwrap();
    data
}

impl TexpressoBundle {
    fn query_server(&mut self, name: &str) -> Answer {
        my_flock(self.lock, libc::LOCK_EX);
        writeln!(self.output, "{}", name).unwrap();
        self.output.flush().unwrap();
        let mut code = [0; 1];
        self.input.read_exact(&mut code).unwrap();
        let answer = match code[0] {
            b'E' => {
                let data = read_data(&mut self.input);
                Answer::E(String::from_utf8(data).unwrap())
            }
            b'P' => {
                let data = read_data(&mut self.input);
                Answer::P(PathBuf::from(std::str::from_utf8(&data).unwrap()))
            }
            b'C' => {
                let data = read_data(&mut self.input);
                Answer::C(data)
            }
            _ => panic!()
        };
        my_flock(self.lock, libc::LOCK_UN);
        answer
    }
}

impl IoProvider for TexpressoBundle {
    fn input_open_name(
        &mut self,
        name: &str,
        _status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        match self.cached.get(name) {
            Some(Some(path)) => {
                let file = File::open(path).unwrap();
                let origin = InputOrigin::Other;
                let input = InputHandle::new_read_only(name, file, origin);
                return OpenResult::Ok(input)
            }
            Some(None) => return OpenResult::NotAvailable,
            None => ()
        };
        match self.query_server(name) {
            Answer::E(str) => {
                eprintln!("[bundle] error loading resource {}: {}", name, str);
                self.cached.insert(name.to_owned(), None);
                OpenResult::NotAvailable
            },
            Answer::P(path) => {
                self.cached.insert(name.to_owned(), Some(path.clone()));
                let file = File::open(path).unwrap();
                let origin = InputOrigin::Other;
                let input = InputHandle::new_read_only(name, file, origin);
                OpenResult::Ok(input)
            },
            Answer::C(data) => {
                eprintln!("[bundle] streaming resource {}", name);
                let file = Cursor::new(data);
                let origin = InputOrigin::Other;
                let input = InputHandle::new_read_only(name, file, origin);
                OpenResult::Ok(input)
            }
        }
    }
}
