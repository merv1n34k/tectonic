//! TODO

// use super::{InputHandle, IoProvider, OpenResult, OutputHandle};
use std::{rc::Rc, cell::{RefCell, RefMut}, io, env};
use libc::wait;
use texpresso_protocol as txp;
use tectonic_status_base::StatusBackend;
use crate::{IoProvider, OpenResult, InputHandle, OutputHandle,
            InputFeatures, SeekFrom, InputOrigin};

/// TODO
pub struct TexpressoIOState {
    client : txp::Client,
    released: Vec<txp::FileId>,
    next_id: txp::FileId,
    last_passed_open: String,
}

/// TODO
pub type TexpressoIOStateRef = Rc<RefCell<TexpressoIOState>>;

/// TODO
pub struct TexpressoIO {
    state: TexpressoIOStateRef,
    primary: String,
}

impl TexpressoIO {
    fn borrow_mut(&self) -> RefMut<'_, TexpressoIOState> {
        self.state.borrow_mut()
    }

    /// TODO
    pub fn new(client: txp::Client, primary: &str) -> TexpressoIO {
        TexpressoIO {
            state: Rc::new(RefCell::new(TexpressoIOState::new(client))),
            primary: primary.into(),
        }
    }

    /// TODO
    pub unsafe fn client_from_env() -> Option<txp::Client> {
        match env::var("TEXPRESSO_FD") {
            Ok(val) => Some(txp::Client::connect_raw_fd(val.parse::<i32>().unwrap())),
            Err(_) => None
        }
    }

    /// TODO
    pub fn new_from_env(primary: &str) -> Option<TexpressoIO> {
        (unsafe {Self::client_from_env()})
            .map(|client| Self::new(client, primary))
    }

    /// TODO
    pub fn stdout(&self) -> TexpressoStdout {
        TexpressoStdout{io: self.state.clone()}
    }

    /// TODO
    pub fn gpic(&mut self, path: &str, typ: i32, page: i32, bounds: &mut [f32; 4]) -> bool {
        self.borrow_mut().client.gpic(path, typ, page, bounds)
    }

    /// TODO
    pub fn spic(&mut self, path: &str, typ: i32, page: i32, bounds: &[f32; 4]) {
        self.borrow_mut().client.spic(path, typ, page, bounds)
    }
}

impl std::clone::Clone for TexpressoIO {
    /// TODO
    fn clone(&self) -> Self {
        let primary = self.primary.clone();
        let state = self.state.clone();
        TexpressoIO {state, primary}
    }
}

/// TODO
pub struct TexpressoReader {
    io: TexpressoIOStateRef,
    id: txp::FileId,
    abs_pos: usize,
    buf: [u8; 1024],
    buf_pos: u32,
    buf_len: u32,
    size: Option<usize>,
    generation: usize,
}

impl TexpressoReader {
    fn get_file_size(&mut self) -> usize {
        match self.size {
            Some(size) => size,
            None => {
                let mut io = self.io.borrow_mut();
                let size = io.client.size(self.id) as usize;
                self.size = Some(size);
                size
            }
        }
    }
}

impl io::Read for TexpressoReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut io = self.io.borrow_mut();
        let generation = io.client.generation();
        if generation != self.generation {
            self.abs_pos += self.buf_pos as usize;
            self.buf_pos = 0;
            self.buf_len = 0;
            self.generation = generation;
        }
        if self.buf_pos == self.buf_len {
            let abs_pos = self.abs_pos + self.buf_pos as usize;
            loop {
                match io.client.read(self.id, abs_pos as u32, &mut self.buf) {
                    Some(size) => {
                        self.abs_pos = abs_pos;
                        self.buf_pos = 0;
                        self.buf_len = size as u32;
                        break;
                    }
                    None => {
                        io.client.flush();
                        io.client.bump_generation();
                        tectonic_geturl::reqwest::clear_shared_client();
                        let child = unsafe { io.client.fork() };
                        if child == 0 {
                            io.client.child(unsafe{libc::getpid()})
                        } else {
                            let mut status : i32 = 1;
                            let result =
                                unsafe { wait(std::ptr::addr_of_mut!(status)) };
                            if result == -1 {
                                panic!("TeXpresso: fork: error while waiting for child");
                            };
                            if result != child {
                                panic!("TeXpresso: fork: unexpected pid");
                            };
                            let resume = io.client.back(unsafe{libc::getpid()}, child, status as u32);
                            if !resume {
                                std::process::exit(1)
                            }
                        }
                    }
                }
            }
        };
        let len = buf.len();
        let pos = self.buf_pos as usize;
        let rem = self.buf_len as usize - pos;
        let n = if rem >= len {
            buf.copy_from_slice(&self.buf[pos..pos + len]);
            self.buf_pos += len as u32;
            len
        } else {
            buf[0..rem].copy_from_slice(&self.buf[pos..pos + rem]);
            self.buf_pos = self.buf_len;
            rem
        };
        io.client.seen(self.id, self.abs_pos as u32 + self.buf_pos);
        Ok(n)
    }

}

impl InputFeatures for TexpressoReader {

    fn get_size(&mut self) -> tectonic_errors::Result<usize> {
        Ok(self.get_file_size())
    }

    fn try_seek(&mut self, pos: SeekFrom) -> tectonic_errors::Result<u64> {
        let size = self.get_file_size();
        let pos = match pos {
            SeekFrom::Start(ofs) =>
                ofs as usize,
            SeekFrom::Current(ofs) =>
                (self.abs_pos as i64 + self.buf_pos as i64+ ofs) as usize,
            SeekFrom::End(ofs) => {
                if ofs as usize > size {
                    panic!("TODO Find a way to return an error :D");
                };
                size - ofs as usize
            }
        };
        if pos > size {
            panic!("TODO Find a way to return an error :D");
        }
        self.abs_pos = pos;
        self.buf_pos = 0;
        self.buf_len = 0;
        Ok(pos as u64)
    }

}

/// TODO
pub struct TexpressoWriter {
    io: TexpressoIOStateRef,
    id: txp::FileId,
    pos: usize,
}

impl TexpressoIOState {
    /// TODO
    pub fn new(client: txp::Client) -> TexpressoIOState {
        TexpressoIOState {
            client,
            released: Vec::new(),
            next_id: 0,
            last_passed_open: "".to_string(),
        }
    }

    /// TODO
    pub unsafe fn new_from_raw_fd(fd: std::os::unix::io::RawFd) -> TexpressoIOState {
        Self::new(txp::Client::connect_raw_fd(fd))
    }

    fn alloc_id(&mut self) -> txp::FileId {
        match self.released.pop() {
            Some(id) => id,
            None => {
                let result = self.next_id;
                if result >= 1024 {
                    panic!("texpresso: Out of file ids");
                };
                self.next_id = result + 1;
                result
            }
        }
    }

    fn release_id(&mut self, id: txp::FileId) {
        // eprintln!("release_id {id}");
        self.released.push(id);
    }
}

impl Drop for TexpressoReader {
    fn drop(&mut self) {
        let mut io = self.io.borrow_mut();
        io.client.close(self.id);
        io.release_id(self.id)
    }
}

impl Drop for TexpressoWriter {
    fn drop(&mut self) {
        let mut io = self.io.borrow_mut();
        io.client.close(self.id);
        io.release_id(self.id)
    }
}

impl io::Write for TexpressoWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut io = self.io.borrow_mut();
        io.client.write(self.id, self.pos as u32, buf);
        let len = buf.len();
        self.pos += len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut io = self.io.borrow_mut();
        io.client.flush();
        Ok(())
    }
}

/// TODO
#[derive(Clone)]
pub struct TexpressoStdout {
    io: TexpressoIOStateRef
}

impl io::Write for TexpressoStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut io = self.io.borrow_mut();
        io.client.write(-1, 0, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut io = self.io.borrow_mut();
        io.client.flush();
        Ok(())
    }
}

impl IoProvider for TexpressoIO {
    /// Open the named file for output.
    fn output_open_name(&mut self, name: &str) -> OpenResult<OutputHandle> {
        let (id, open) = {
            let mut io = self.borrow_mut();
            let id = io.alloc_id();
            let open = io.client.open(id, name, "w");
            if open.is_none() { io.release_id(id); };
            io.last_passed_open = "".to_string();
            (id, open)
        };
        match open {
            None => OpenResult::NotAvailable,
            Some(path) => {
                let writer = TexpressoWriter{io: self.state.clone(), id, pos: 0};
                OpenResult::Ok(OutputHandle::new_without_digest(path, writer))
            }
        }
    }

    /// Open the standard output stream.
    fn output_open_stdout(&mut self) -> OpenResult<OutputHandle> {
        OpenResult::NotAvailable
        // OpenResult::Ok(OutputHandle::new_without_digest("stdout", self.stdout()))
    }

    /// Open the named file for input.
    fn input_open_name(
        &mut self,
        name: &str,
        _status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        let (id, open, generation) = {
            let mut io = self.borrow_mut();
            if name == io.last_passed_open {
                return OpenResult::NotAvailable
            };
            let id = io.alloc_id();
            let open = io.client.open(id, name, "r?");
            if open.is_none() {
                io.last_passed_open = name.to_string();
                io.release_id(id);
            } else {
                io.last_passed_open = "".to_string();
            };
            (id, open, io.client.generation())
        };
        match open {
            | None => OpenResult::NotAvailable,
            | Some(path) => {
                let reader = TexpressoReader {
                    io: self.state.clone(),
                    id, generation,
                    abs_pos: 0,
                    buf: [0; 1024],
                    buf_pos: 0,
                    buf_len: 0,
                    size: None,
                };
                OpenResult::Ok(InputHandle::new_read_only(path, reader, InputOrigin::Other))
            }
        }
    }

    fn input_open_format(
        &mut self,
        _name: &str,
        _status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        OpenResult::NotAvailable
    }

    fn input_open_primary(&mut self, status: &mut dyn StatusBackend) -> OpenResult<InputHandle> {
        self.input_open_name(&self.primary.clone(), status)
    }

    fn input_open_primary_with_abspath(
        &mut self,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<(InputHandle, Option<std::path::PathBuf>)> {
        match self.input_open_primary(status) {
            OpenResult::Ok(ih) => OpenResult::Ok((ih, Some(self.primary.clone().into()))),
            OpenResult::Err(x) => OpenResult::Err(x),
            OpenResult::NotAvailable => OpenResult::NotAvailable,
        }
    }

    fn input_open_name_with_abspath(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<(InputHandle, Option<std::path::PathBuf>)> {
        match self.input_open_name(name, status) {
            OpenResult::Ok(ih) => OpenResult::Ok((ih, Some(name.into()))),
            OpenResult::Err(x) => OpenResult::Err(x),
            OpenResult::NotAvailable => OpenResult::NotAvailable,
        }
    }
}
