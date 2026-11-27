//! Minimal NBT (Named Binary Tag) reader for blueprints (.schem/.litematic).
//!
//! Detects gzip/zlib/uncompressed automatically. Read-only: writing is not supported.

use flate2::read::{GzDecoder, ZlibDecoder};
use std::collections::HashMap;
use std::io::Read;

#[derive(Debug, Clone)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Nbt>),
    Compound(HashMap<String, Nbt>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    End,
}

impl Nbt {
    pub fn as_compound(&self) -> Option<&HashMap<String, Nbt>> {
        if let Nbt::Compound(m) = self { Some(m) } else { None }
    }
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        self.as_compound().and_then(|m| m.get(key))
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Nbt::Byte(v) => Some(*v as i32),
            Nbt::Short(v) => Some(*v as i32),
            Nbt::Int(v) => Some(*v),
            Nbt::Long(v) => Some(*v as i32),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Nbt::String(s) = self { Some(s) } else { None }
    }
    pub fn as_byte_array(&self) -> Option<&[u8]> {
        if let Nbt::ByteArray(b) = self { Some(b) } else { None }
    }
    pub fn as_long_array(&self) -> Option<&[i64]> {
        if let Nbt::LongArray(l) = self { Some(l) } else { None }
    }
    pub fn as_list(&self) -> Option<&[Nbt]> {
        if let Nbt::List(l) = self { Some(l) } else { None }
    }
}

/// Largest decompressed blueprint accepted (a gzip bomb must not eat the machine).
pub const MAX_DECOMPRESSED: u64 = 128 << 20;
/// Most tags in one document: every tag costs tens of bytes in memory.
const MAX_TAGS: usize = 4_000_000;
/// Vanilla's own nesting limit; deeper input would overflow the stack.
const MAX_DEPTH: u32 = 512;

/// Detect compression and read the NBT root. Returns (root name, value).
pub fn read(bytes: &[u8]) -> Result<(String, Nbt), String> {
    let data = decompress(bytes)?;
    let mut cur = Cursor { data: &data, pos: 0, depth: 0, tags: 0 };
    let tag = cur.u8()?;
    if tag == 0 {
        return Ok((String::new(), Nbt::End));
    }
    let name = cur.string()?;
    let value = cur.payload(tag)?;
    Ok((name, value))
}

/// Network NBT as used by play/configuration packets since 1.20.2: the root tag has no name.
pub fn read_network(bytes: &[u8]) -> Result<Nbt, String> {
    let mut cur = Cursor { data: bytes, pos: 0, depth: 0, tags: 0 };
    let tag = cur.u8()?;
    if tag == 0 {
        return Ok(Nbt::End);
    }
    cur.payload(tag)
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        inflate(GzDecoder::new(bytes), "gzip")
    } else if bytes.len() >= 2 && bytes[0] == 0x78 {
        inflate(ZlibDecoder::new(bytes), "zlib")
    } else {
        Ok(bytes.to_vec())
    }
}

fn inflate(r: impl Read, what: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    r.take(MAX_DECOMPRESSED + 1).read_to_end(&mut out).map_err(|e| format!("{what}: {e}"))?;
    if out.len() as u64 > MAX_DECOMPRESSED {
        return Err(format!("{what}: larger than {} MB decompressed", MAX_DECOMPRESSED >> 20));
    }
    Ok(out)
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    depth: u32,
    tags: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.data.len() - self.pos {
            return Err("NBT: unexpected end of data".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let len = self.i16()? as usize;
        let b = self.take(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    /// An array length, refused when the rest of the input cannot hold that many elements
    /// (so a forged length never allocates).
    fn len(&mut self, element: usize) -> Result<usize, String> {
        let n = self.i32()?.max(0) as usize;
        if n > (self.data.len() - self.pos) / element {
            return Err("NBT: array longer than the data".into());
        }
        Ok(n)
    }

    fn payload(&mut self, tag: u8) -> Result<Nbt, String> {
        self.tags += 1;
        self.depth += 1;
        if self.depth > MAX_DEPTH || self.tags > MAX_TAGS {
            return Err("NBT: nested too deeply or too many tags".into());
        }
        let value = self.value(tag);
        self.depth -= 1;
        value
    }

    fn value(&mut self, tag: u8) -> Result<Nbt, String> {
        Ok(match tag {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            6 => Nbt::Double(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            7 => {
                let len = self.len(1)?;
                Nbt::ByteArray(self.take(len)?.to_vec())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let len = self.len(1)?;
                let mut list = Vec::new();
                for _ in 0..len {
                    list.push(self.payload(elem)?);
                }
                Nbt::List(list)
            }
            10 => {
                let mut map = HashMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let v = self.payload(t)?;
                    map.insert(name, v);
                }
                Nbt::Compound(map)
            }
            11 => {
                let len = self.len(4)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.i32()?);
                }
                Nbt::IntArray(v)
            }
            12 => {
                let len = self.len(8)?;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.i64()?);
                }
                Nbt::LongArray(v)
            }
            other => return Err(format!("NBT: unknown tag {other}")),
        })
    }
}
