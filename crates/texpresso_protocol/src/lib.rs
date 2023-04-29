use cpu_time::ProcessTime;
use libc;
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::prelude::FromRawFd,
    time::Duration,
};

pub type FileId = i32;
pub type ClientId = i32;

const BUF_SIZE : usize = 4096;

pub struct ClientIO {
    file: File,
    start_time: ProcessTime,
    delta: Duration,
}

pub struct Client {
    io: ClientIO,
    seen: FileId,
    seen_pos: u32,
    write: FileId,
    write_buf: [u8; BUF_SIZE],
    write_pos: u32,
    write_len: u32,
}

fn write_or_panic(file: &mut File, data: &[u8]) -> () {
    match file.write(data) {
        Ok(n) => {
            if n != data.len() {
                panic!("Texpresso: wrote only {n} bytes out of {}", data.len());
            }
        }
        Err(error) => panic!("Texpresso: cannot write to server ({error})"),
    }
}

pub enum AccessResult {
    Pass,
    Ok,
    ENoEnt,
    EAccess,
}

impl ClientIO {
    fn connect(mut file: File) -> ClientIO {
        write_or_panic(&mut file, b"TEXPRESSOC01");
        file.flush().unwrap();
        let mut buf = [0; 12];
        file.read_exact(&mut buf).unwrap();
        if !buf.eq(b"TEXPRESSOS01") {
            panic!("Texpresso connect: invalid handshake")
        };
        eprintln!("texpresso: handshake success");
        ClientIO {
            file,
            start_time: ProcessTime::now(),
            delta: Duration::ZERO,
        }
    }

    fn send_str(&mut self, text: &str) -> () {
        write_or_panic(&mut self.file, text.as_bytes());
        write_or_panic(&mut self.file, b"\x00");
    }

    fn send4(&mut self, data: [u8; 4]) -> () {
        write_or_panic(&mut self.file, &data)
    }

    fn send_tag(&mut self, tag: [u8; 4]) -> () {
        self.send4(tag);
        let time = self.delta + ProcessTime::elapsed(&self.start_time);
        self.send4((time.as_millis() as u32).to_le_bytes());
    }

    fn flush(&mut self) {
        self.file.flush().unwrap();
    }

    fn recv4(&mut self) -> [u8; 4] {
        let mut result = [0; 4];
        self.flush();
        self.file.read_exact(&mut result).unwrap();
        result
    }

    fn recv_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.recv4())
    }

    fn recv_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.recv4())
    }

    fn recv_tag(&mut self) -> [u8; 4] {
        match &self.recv4() {
            b"TERM" => {
                let expected_pid = self.recv_i32();
                let self_pid = unsafe {libc::getpid()};
                if expected_pid != self_pid {
                    panic!("TeXpresso terminate: process pid is {self_pid}, \
                            expected {expected_pid}")
                };
                std::process::exit(1)
            }
            tag => *tag
        }
    }

    fn check_done(&mut self) -> () {
        match &self.recv_tag() {
            b"DONE" => (),
            _ => panic!(),
        }
    }

    fn open(&mut self, file: FileId, path: &str, mode: &str) -> bool {
        self.send_tag(*b"OPEN");
        self.send4(file.to_le_bytes());
        self.send_str(path);
        self.send_str(mode);
        match &self.recv_tag() {
            b"DONE" => return true,
            b"PASS" => return false,
            _ => panic!(),
        };
    }

    fn read(&mut self, file: FileId, pos: u32, buf: &mut [u8]) -> Option<usize> {
        self.send_tag(*b"READ");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
        self.send4((buf.len() as u32).to_le_bytes());

        match &self.recv_tag() {
            b"FORK" => return None,
            b"READ" => {
                let rd_size = self.recv_u32() as usize;
                if rd_size > buf.len() {
                    panic!()
                };
                self.file.read_exact(&mut buf[..rd_size]).unwrap();
                return Some(rd_size);
            }
            _ => panic!(),
        };
    }

    fn write(&mut self, file: FileId, pos: u32, b1: &[u8], b2: &[u8]) {
        self.send_tag(*b"WRIT");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
        let len = b1.len() + b2.len();
        self.send4((len as u32).to_le_bytes());
        self.file.write_all(b1).unwrap();
        self.file.write_all(b2).unwrap();
        self.check_done();
    }

    fn close(&mut self, file: FileId) -> () {
        self.send_tag(*b"CLOS");
        self.send4(file.to_le_bytes());
        self.check_done();
    }

    fn size(&mut self, file: FileId) -> u32 {
        self.send_tag(*b"SIZE");
        self.send4(file.to_le_bytes());
        match &self.recv_tag() {
            b"SIZE" => return self.recv_u32(),
            _ => panic!(),
        }
    }

    fn seen(&mut self, file: FileId, pos: u32) {
        self.send_tag(*b"SEEN");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
    }

    fn child(&mut self, id: ClientId) {
        self.send_tag(*b"CHLD");
        self.send4(id.to_le_bytes());
        self.check_done();
    }

    fn back(&mut self, id: ClientId, child: ClientId, exitcode: u32) -> bool {
        self.send_tag(*b"BACK");
        self.send4(id.to_le_bytes());
        self.send4(child.to_le_bytes());
        self.send4(exitcode.to_le_bytes());
        match &self.recv_tag() {
            b"DONE" => return true,
            b"PASS" => return false,
            _ => panic!(),
        }
    }

    fn accs(&mut self, path: &str, read: bool, write: bool, execute: bool) -> AccessResult {
        let mut mode: u32 = 0;
        if read {
            mode |= 1
        };
        if write {
            mode |= 2
        };
        if execute {
            mode |= 4
        };

        self.send_tag(*b"ACCS");
        self.send_str(path);
        self.send4(mode.to_le_bytes());

        match &self.recv_tag() {
            b"ACCS" => match self.recv_u32() {
                0 => return AccessResult::Pass,
                1 => return AccessResult::Ok,
                2 => return AccessResult::ENoEnt,
                3 => return AccessResult::EAccess,
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    fn stat(&mut self, path: &str, stat: &mut libc::stat) -> AccessResult {
        self.send_tag(*b"STAT");
        self.send_str(path);

        match &self.recv_tag() {
            b"STAT" => (),
            _ => panic!(),
        };

        match self.recv_u32() {
            0 => return AccessResult::Pass,
            1 => {
                stat.st_dev = self.recv_i32() as libc::dev_t;
                stat.st_ino = self.recv_u32() as libc::ino_t;
                stat.st_mode = self.recv_u32() as libc::mode_t;
                stat.st_nlink = self.recv_u32() as libc::nlink_t;
                stat.st_uid = self.recv_u32() as libc::uid_t;
                stat.st_gid = self.recv_u32() as libc::gid_t;
                stat.st_rdev = self.recv_i32() as libc::dev_t;
                stat.st_size = self.recv_u32() as libc::off_t;
                stat.st_blksize = self.recv_i32() as libc::blksize_t;
                stat.st_blocks = self.recv_u32() as libc::blkcnt_t;
                stat.st_atime = self.recv_u32() as libc::time_t;
                stat.st_atime_nsec = self.recv_u32() as i64;
                stat.st_ctime = self.recv_u32() as libc::time_t;
                stat.st_ctime_nsec = self.recv_u32() as i64;
                stat.st_mtime = self.recv_u32() as libc::time_t;
                stat.st_mtime_nsec = self.recv_u32() as i64;
                return AccessResult::Ok;
            }
            2 => return AccessResult::ENoEnt,
            3 => return AccessResult::EAccess,
            _ => panic!(),
        }
    }

    unsafe fn fork(&mut self) -> libc::pid_t {
        self.flush();
        let time = ProcessTime::elapsed(&self.start_time);
        let result = libc::fork();
        if result == 0 {
            self.delta += time;
            self.start_time = ProcessTime::now();
        };
        return result;
    }
}

impl Client {
    pub fn connect(file: File) -> Client {
        let io = ClientIO::connect(file);
        Client {
            io,
            seen: -1,
            seen_pos: 0,
            write: -1,
            write_buf: [0; BUF_SIZE],
            write_pos: 0,
            write_len: 0,
        }
    }

    pub unsafe fn connect_raw_fd(fd: std::os::unix::io::RawFd) -> Client {
        Self::connect(File::from_raw_fd(fd))
    }

    fn flush_pending(&mut self) {
        if self.seen_pos != 0 {
            self.io.seen(self.seen, self.seen_pos);
            self.seen_pos = 0;
            self.seen = -1;
        }
        if self.write_len != 0 {
            let len = self.write_len as usize;
            self.io.write(self.write, self.write_pos, &self.write_buf[0..len], &[]);
            self.write_len = 0;
            self.write = -1;
        }
    }

    pub fn flush(&mut self) {
        self.flush_pending();
        self.io.flush()
    }

    pub fn open(&mut self, file: FileId, path: &str, mode: &str) -> bool {
        //eprintln!("open({file}, {path}, {mode})");
        self.flush_pending();
        self.io.open(file, path, mode)
    }

    pub fn read(&mut self, file: FileId, pos: u32, buf: &mut [u8]) -> Option<usize> {
        self.flush_pending();
        self.io.read(file, pos, buf)
    }

    pub fn write(&mut self, file: FileId, pos: u32, buf: &[u8]) -> () {
        if self.write_len > 0 {
            let abs_pos = self.write_pos + self.write_len;
            if self.write != file || abs_pos != pos {
                self.flush_pending();
                self.write = file;
                self.write_pos = pos;
            }
        } else {
            self.write = file;
            self.write_pos = pos;
        }
        if (self.write_len as usize + buf.len()) <= BUF_SIZE {
            let ofs = self.write_len as usize;
            let lim = ofs + buf.len();
            self.write_buf[ofs .. lim].copy_from_slice(buf);
            self.write_len = lim as u32;
        } else {
            self.io.write(file, self.write_pos, &self.write_buf[0 .. self.write_len as usize], buf);
            self.write_len = 0;
        }
    }

    pub fn close(&mut self, file: FileId) -> () {
        self.flush_pending();
        self.io.close(file)
    }

    pub fn size(&mut self, file: FileId) -> u32 {
        self.flush_pending();
        self.io.size(file)
    }

    pub fn seen(&mut self, file: FileId, pos: u32) {
        if self.seen != file {
            self.flush_pending();
            self.seen = file;
        };
        if self.seen_pos < pos {
            self.seen_pos = pos;
        }
    }

    pub fn child(&mut self, id: ClientId) {
        if self.write_len > 0 || self.seen_pos > 0 {
            panic!("texpresso child: expecting empty buffer");
        }
        self.io.child(id)
    }

    pub fn back(&mut self, id: ClientId, child: ClientId, exitcode: u32) -> bool {
        if self.write_len > 0 || self.seen_pos > 0 {
            panic!("texpresso back: expecting empty buffer");
        }
        self.io.back(id, child, exitcode)
    }

    pub fn accs(&mut self, path: &str, read: bool, write: bool, execute: bool) -> AccessResult {
        self.flush_pending();
        self.io.accs(path, read, write, execute)
    }

    pub fn stat(&mut self, path: &str, stat: &mut libc::stat) -> AccessResult {
        self.flush_pending();
        self.io.stat(path, stat)
    }

    pub unsafe fn fork(&mut self) -> libc::pid_t {
        self.flush_pending();
        self.io.fork()
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
