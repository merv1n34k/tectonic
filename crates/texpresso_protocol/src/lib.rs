use cpu_time::ProcessTime;
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::prelude::FromRawFd,
    os::fd::AsRawFd,
    time::Duration,
};

pub type FileId = i32;
pub type ClientId = i32;

const BUF_SIZE : usize = 4096;

pub struct ClientIO {
    file: File,
    start_time: ProcessTime,
    delta: Duration,
    generation: usize,
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

fn fmt_tag(tag: &[u8; 4]) -> [char; 4] {
    [tag[0] as char, tag[1] as char, tag[2] as char, tag[3] as char]
}

fn write_or_panic(file: &mut File, data: &[u8]) {
    match file.write(data) {
        Ok(n) => {
            if n != data.len() {
                panic!("TeXpresso: wrote only {n} bytes out of {}", data.len());
            }
        }
        Err(error) => {
            eprintln!("TeXpresso: cannot write to server ({error})");
            std::process::exit(1)
        }
    }
}

extern "C" {
    fn texpresso_fork_with_channel(
        fd: libc::c_int,
        time: u32
        ) -> libc::c_int;
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
            generation: 0,
        }
    }

    fn send_str(&mut self, text: &str) {
        write_or_panic(&mut self.file, text.as_bytes());
        write_or_panic(&mut self.file, b"\x00");
    }

    fn send4(&mut self, data: [u8; 4]) {
        write_or_panic(&mut self.file, &data)
    }

    fn send_tag(&mut self, tag: [u8; 4]) {
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
        match self.file.read_exact(&mut result) {
            Ok(()) => result,
            Err(error) => {
                eprintln!("TeXpresso: cannot read from server ({error})");
                std::process::exit(1)
            }
        }
    }

    fn recv_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.recv4())
    }

    fn recv_f32(&mut self) -> f32 {
        f32::from_le_bytes(self.recv4())
    }

    fn recv_tag(&mut self) -> [u8; 4] {
        match &self.recv4() {
            b"FLSH" => {
                self.generation += 1;
                self.recv_tag()
            }
            tag => *tag
        }
    }

    fn check_done(&mut self) {
        match &self.recv_tag() {
            b"DONE" => (),
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
    }

    fn open(&mut self, file: FileId, path: &str, mode: &str) -> Option<String> {
        self.send_tag(*b"OPEN");
        self.send4(file.to_le_bytes());
        self.send_str(path);
        self.send_str(mode);
        match &self.recv_tag() {
            b"PASS" => None,
            b"OPEN" => {
                let size = self.recv_u32() as usize;
                let mut buf = vec![0u8; size];
                self.file.read_exact(&mut buf).unwrap();
                Some(String::from_utf8(buf).unwrap())
            },
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
    }

    fn read(&mut self, file: FileId, pos: u32, buf: &mut [u8]) -> Option<usize> {
        self.send_tag(*b"READ");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
        self.send4((buf.len() as u32).to_le_bytes());

        match &self.recv_tag() {
            b"FORK" => None,
            b"READ" => {
                let rd_size = self.recv_u32() as usize;
                if rd_size > buf.len() {
                    panic!()
                };
                self.file.read_exact(&mut buf[..rd_size]).unwrap();
                Some(rd_size)
            }
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
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

    fn close(&mut self, file: FileId) {
        self.send_tag(*b"CLOS");
        self.send4(file.to_le_bytes());
        self.check_done();
    }

    fn size(&mut self, file: FileId) -> u32 {
        self.send_tag(*b"SIZE");
        self.send4(file.to_le_bytes());
        match &self.recv_tag() {
            b"SIZE" => self.recv_u32(),
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
    }

    fn seen(&mut self, file: FileId, pos: u32) {
        self.send_tag(*b"SEEN");
        self.send4(file.to_le_bytes());
        self.send4(pos.to_le_bytes());
    }

    unsafe fn fork(&mut self) -> libc::pid_t {
        self.flush();
        let delta = self.delta + ProcessTime::elapsed(&self.start_time);
        let result = texpresso_fork_with_channel(self.file.as_raw_fd(), delta.as_millis() as u32);
        if result == 0 {
            self.delta = delta;
            self.start_time = ProcessTime::now();
        };
        result
    }

    fn gpic(&mut self, path: &str, typ: i32, page: i32, bounds: &mut [f32; 4]) -> bool {
        self.send_tag(*b"GPIC");
        self.send_str(path);
        self.send4(typ.to_le_bytes());
        self.send4(page.to_le_bytes());
        match &self.recv_tag() {
            b"PASS" => false,
            b"GPIC" => {
                bounds[0] = self.recv_f32();
                bounds[1] = self.recv_f32();
                bounds[2] = self.recv_f32();
                bounds[3] = self.recv_f32();
                true
            },
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
    }

    fn spic(&mut self, path: &str, typ: i32, page: i32, bounds: &[f32; 4]) {
        self.send_tag(*b"SPIC");
        self.send_str(path);
        self.send4(typ.to_le_bytes());
        self.send4(page.to_le_bytes());
        self.send4(bounds[0].to_le_bytes());
        self.send4(bounds[1].to_le_bytes());
        self.send4(bounds[2].to_le_bytes());
        self.send4(bounds[3].to_le_bytes());
        match &self.recv_tag() {
            b"DONE" => (),
            tag => panic!("TeXpresso: unexpected tag {:?}", fmt_tag(tag)),
        }
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

    pub fn generation(&self) -> usize {
        self.io.generation
    }

    pub fn bump_generation(&mut self) {
        self.io.generation += 1
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

    pub fn open(&mut self, file: FileId, path: &str, mode: &str) -> Option<String> {
        //eprintln!("open({file}, {path}, {mode})");
        self.flush_pending();
        self.io.open(file, path, mode)
    }

    pub fn read(&mut self, file: FileId, pos: u32, buf: &mut [u8]) -> Option<usize> {
        self.flush_pending();
        self.io.read(file, pos, buf)
    }

    pub fn write(&mut self, file: FileId, pos: u32, buf: &[u8]) {
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

    pub fn close(&mut self, file: FileId) {
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

    pub unsafe fn fork(&mut self) -> libc::pid_t {
        self.flush_pending();
        self.io.fork()
    }

    pub fn gpic(&mut self, path: &str, typ: i32, page: i32, bounds: &mut [f32; 4]) -> bool {
        self.flush_pending();
        self.io.gpic(path, typ, page, bounds)
    }

    pub fn spic(&mut self, path: &str, typ: i32, page: i32, bounds: &[f32; 4]) {
        self.flush_pending();
        self.io.spic(path, typ, page, bounds)
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
