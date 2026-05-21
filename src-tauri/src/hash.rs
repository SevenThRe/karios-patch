use crate::error::AppResult;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::{fs::File, io::Read, path::Path};

pub fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    sha256_reader(&mut file)
}

pub fn sha256_reader(reader: &mut impl Read) -> AppResult<String> {
    hash_reader(reader, Sha256::new())
}

pub fn sha512_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    sha512_reader(&mut file)
}

pub fn sha512_reader(reader: &mut impl Read) -> AppResult<String> {
    hash_reader(reader, Sha512::new())
}

pub fn sha1_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    sha1_reader(&mut file)
}

pub fn sha1_reader(reader: &mut impl Read) -> AppResult<String> {
    hash_reader(reader, Sha1::new())
}

fn hash_reader<D: Digest>(reader: &mut impl Read, mut hasher: D) -> AppResult<String> {
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex_lower(&hasher.finalize()))
}

#[cfg(test)]
pub fn sha512_bytes(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha512::digest(bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
