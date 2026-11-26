//! 최소 NBT(Named Binary Tag) 리더 — 설계도(.schem/.litematic)를 읽기 위한 것.
//!
//! gzip/zlib/비압축을 자동 감지한다. 쓰기는 지원하지 않는다(읽기 전용).

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

/// 압축을 자동 감지해 NBT 루트를 읽는다. 반환: (루트 이름, 값).
pub fn read(bytes: &[u8]) -> Result<(String, Nbt), String> {
    let data = decompress(bytes)?;
    let mut cur = Cursor { data: &data, pos: 0 };
    let tag = cur.u8()?;
    if tag == 0 {
        return Ok((String::new(), Nbt::End));
    }
    let name = cur.string()?;
    let value = cur.payload(tag)?;
    Ok((name, value))
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut out = Vec::new();
        GzDecoder::new(bytes).read_to_end(&mut out).map_err(|e| format!("gzip: {e}"))?;
        Ok(out)
    } else if bytes.len() >= 2 && bytes[0] == 0x78 {
        let mut out = Vec::new();
        ZlibDecoder::new(bytes).read_to_end(&mut out).map_err(|e| format!("zlib: {e}"))?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err("NBT: 데이터 부족".into());
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

    fn payload(&mut self, tag: u8) -> Result<Nbt, String> {
        Ok(match tag {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            6 => Nbt::Double(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            7 => {
                let len = self.i32()?.max(0) as usize;
                Nbt::ByteArray(self.take(len)?.to_vec())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let len = self.i32()?.max(0) as usize;
                let mut list = Vec::with_capacity(len);
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
                let len = self.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.i32()?);
                }
                Nbt::IntArray(v)
            }
            12 => {
                let len = self.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(len);
                for _ in 0..len {
                    v.push(self.i64()?);
                }
                Nbt::LongArray(v)
            }
            other => return Err(format!("NBT: 알 수 없는 태그 {other}")),
        })
    }
}
