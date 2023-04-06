use cpu_time::ProcessTime;
use libc;
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::prelude::FromRawFd,
    time::Duration,
};

pub struct Client {
    file: File,
    start_time: ProcessTime,
    delta: Duration,
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

pub type FileId = i32;
pub type ClientId = i32;

pub enum AccessResult {
    Pass,
    Ok,
    ENoEnt,
    EAccess,
}

impl Client {
    pub fn connect(mut file: File) -> Client {
        write_or_panic(&mut file, b"TEXPRESSOC01");
        file.flush().unwrap();
        let mut buf = [0; 12];
        file.read_exact(&mut buf).unwrap();
        if !buf.eq(b"TEXPRESSOS01") {
            panic!("Texpresso connect: invalid handshake")
        };
        eprintln!("texpresso: handshake success");
        Client {
            file,
            start_time: ProcessTime::now(),
            delta: Duration::ZERO,
        }
    }

    pub unsafe fn connect_raw_fd(fd: std::os::unix::io::RawFd) -> Client {
        Self::connect(File::from_raw_fd(fd))
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

    fn recv4(&mut self) -> [u8; 4] {
        let mut result = [0; 4];
        self.file.flush().unwrap();
        self.file.read_exact(&mut result).unwrap();
        result
    }

    fn check_done(&mut self) -> () {
        match &self.recv4() {
            b"DONE" => (),
            _ => panic!(),
        }
    }

    pub fn open(&mut self, file: FileId, path: &str, mode: &str) -> bool {
        eprintln!("open({file}, {path}, {mode})");
        self.send_tag(*b"OPEN");
        self.send4(file.to_le_bytes());
        self.send_str(path);
        self.send_str(mode);
        match &self.recv4() {
            b"DONE" => return true,
            b"PASS" => return false,
            _ => panic!(),
        };
    }

    pub fn read(&mut self, file: FileId, pos: u32, buf: &mut [u8]) -> Option<usize> {
        self.send_tag(*b"READ");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
        self.send4((buf.len() as u32).to_le_bytes());

        match &self.recv4() {
            b"FORK" => return None,
            b"READ" => {
                let rd_size = u32::from_le_bytes(self.recv4()) as usize;
                if rd_size > buf.len() {
                    panic!()
                };
                self.file.read_exact(&mut buf[..rd_size]).unwrap();
                return Some(rd_size);
            }
            _ => panic!(),
        };
    }

    pub fn write(&mut self, file: FileId, pos: u32, buf: &[u8]) -> () {
        self.send_tag(*b"WRIT");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
        self.send4((buf.len() as u32).to_le_bytes());
        self.file.write_all(buf).unwrap();
        self.check_done();
    }

    pub fn close(&mut self, file: FileId) -> () {
        self.send_tag(*b"CLOS");
        self.send4(file.to_le_bytes());
        self.check_done();
    }

    pub fn size(&mut self, file: FileId) -> u32 {
        self.send_tag(*b"SIZE");
        self.send4(file.to_le_bytes());
        match &self.recv4() {
            b"SIZE" => return u32::from_le_bytes(self.recv4()),
            _ => panic!(),
        }
    }

    pub fn seen(&mut self, file: FileId, pos: u32) {
        self.send_tag(*b"SEEN");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
    }

    pub fn child(&mut self, id: ClientId) {
        self.send_tag(*b"CHLD");
        self.send4(id.to_le_bytes());
        self.check_done();
    }

    pub fn back(&mut self, id: ClientId, child: ClientId, exitcode: u32) -> bool {
        self.send_tag(*b"BACK");
        self.send4(id.to_le_bytes());
        self.send4(child.to_le_bytes());
        self.send4(exitcode.to_le_bytes());
        match &self.recv4() {
            b"DONE" => return true,
            b"PASS" => return false,
            _ => panic!(),
        }
    }

    pub fn accs(&mut self, path: &str, read: bool, write: bool, execute: bool) -> AccessResult {
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

        match &self.recv4() {
            b"ACCS" => match u32::from_le_bytes(self.recv4()) {
                0 => return AccessResult::Pass,
                1 => return AccessResult::Ok,
                2 => return AccessResult::ENoEnt,
                3 => return AccessResult::EAccess,
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    pub fn stat(&mut self, path: &str, mut stat: libc::stat) -> AccessResult {
        self.send_tag(*b"STAT");
        self.send_str(path);

        match &self.recv4() {
            b"STAT" => (),
            _ => panic!(),
        };

        match u32::from_le_bytes(self.recv4()) {
            0 => return AccessResult::Pass,
            1 => {
                stat.st_dev = i32::from_le_bytes(self.recv4());
                stat.st_ino = u32::from_le_bytes(self.recv4()) as u64;
                stat.st_mode = u32::from_le_bytes(self.recv4()) as u16;
                stat.st_nlink = u32::from_le_bytes(self.recv4()) as u16;
                stat.st_uid = u32::from_le_bytes(self.recv4());
                stat.st_gid = u32::from_le_bytes(self.recv4());
                stat.st_rdev = i32::from_le_bytes(self.recv4());
                stat.st_size = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_blksize = i32::from_le_bytes(self.recv4());
                stat.st_blocks = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_atime = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_atime_nsec = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_ctime = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_ctime_nsec = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_mtime = u32::from_le_bytes(self.recv4()) as i64;
                stat.st_mtime_nsec = u32::from_le_bytes(self.recv4()) as i64;
                return AccessResult::Ok;
            }
            2 => return AccessResult::ENoEnt,
            3 => return AccessResult::EAccess,
            _ => panic!(),
        }
    }

    pub unsafe fn fork(&mut self) -> libc::pid_t {
        let time = ProcessTime::elapsed(&self.start_time);
        let result = libc::fork();
        if result == 0 {
            self.delta += time;
            self.start_time = ProcessTime::now();
        };
        return result;
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
