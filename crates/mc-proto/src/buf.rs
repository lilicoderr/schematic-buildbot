//! 패킷 바이트 버퍼 — Minecraft 와이어 타입 읽기/쓰기.
//!
//! Minecraft 프로토콜의 정수는 빅엔디언, VarInt/VarLong 은 LEB128(부호는 2의 보수 비트패턴).
//! Position 은 x(26)·z(26)·y(12) 를 i64 하나에 패킹한다.

use std::fmt;

#[derive(Debug)]
pub enum ProtoError {
    Eof,
    VarIntTooLong,
    BadUtf8,
    BadValue(String),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Eof => write!(f, "버퍼 끝에 도달 (데이터 부족)"),
            ProtoError::VarIntTooLong => write!(f, "VarInt 가 너무 김"),
            ProtoError::BadUtf8 => write!(f, "잘못된 UTF-8 문자열"),
            ProtoError::BadValue(s) => write!(f, "잘못된 값: {s}"),
        }
    }
}
impl std::error::Error for ProtoError {}

pub type Result<T> = std::result::Result<T, ProtoError>;

/// 읽기 커서를 가진 바이트 버퍼. 쓰기는 항상 끝에 덧붙인다.
#[derive(Default, Clone)]
pub struct PacketBuf {
    data: Vec<u8>,
    pos: usize,
}

impl PacketBuf {
    pub fn new() -> Self {
        Self { data: Vec::new(), pos: 0 }
    }
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn position(&self) -> usize {
        self.pos
    }

    // ── 저수준 ──
    pub fn write_bytes(&mut self, b: &[u8]) {
        self.data.extend_from_slice(b);
    }
    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        if self.remaining() < n {
            return Err(ProtoError::Eof);
        }
        let out = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(out)
    }
    pub fn read_rest(&mut self) -> Vec<u8> {
        let out = self.data[self.pos..].to_vec();
        self.pos = self.data.len();
        out
    }

    pub fn write_u8(&mut self, v: u8) {
        self.data.push(v);
    }
    pub fn read_u8(&mut self) -> Result<u8> {
        if self.remaining() < 1 {
            return Err(ProtoError::Eof);
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn write_bool(&mut self, v: bool) {
        self.write_u8(v as u8);
    }
    pub fn read_bool(&mut self) -> Result<bool> {
        Ok(self.read_u8()? != 0)
    }

    pub fn write_i8(&mut self, v: i8) {
        self.write_u8(v as u8);
    }
    pub fn read_i8(&mut self) -> Result<i8> {
        Ok(self.read_u8()? as i8)
    }

    // 빅엔디언 고정폭 정수
    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.read_bytes(2)?.try_into().unwrap()))
    }
    pub fn write_i16(&mut self, v: i16) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.read_bytes(2)?.try_into().unwrap()))
    }
    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.read_bytes(4)?.try_into().unwrap()))
    }
    pub fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }
    pub fn write_f32(&mut self, v: f32) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.read_bytes(4)?.try_into().unwrap()))
    }
    pub fn write_f64(&mut self, v: f64) {
        self.write_bytes(&v.to_be_bytes());
    }
    pub fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_be_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }

    // ── VarInt / VarLong (LEB128, 부호는 2의 보수 비트패턴) ──
    pub fn write_varint(&mut self, value: i32) {
        let mut v = value as u32;
        loop {
            if v & !0x7F == 0 {
                self.write_u8(v as u8);
                break;
            }
            self.write_u8(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
    }
    pub fn read_varint(&mut self) -> Result<i32> {
        let mut result: u32 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 35 {
                return Err(ProtoError::VarIntTooLong);
            }
        }
        Ok(result as i32)
    }
    pub fn varint_len(value: i32) -> usize {
        let mut v = value as u32;
        let mut n = 1;
        while v & !0x7F != 0 {
            v >>= 7;
            n += 1;
        }
        n
    }

    pub fn write_varlong(&mut self, value: i64) {
        let mut v = value as u64;
        loop {
            if v & !0x7F == 0 {
                self.write_u8(v as u8);
                break;
            }
            self.write_u8(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
    }
    pub fn read_varlong(&mut self) -> Result<i64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.read_u8()?;
            result |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 70 {
                return Err(ProtoError::VarIntTooLong);
            }
        }
        Ok(result as i64)
    }

    // ── 문자열 (VarInt 길이 + UTF-8) ──
    pub fn write_string(&mut self, s: &str) {
        self.write_varint(s.len() as i32);
        self.write_bytes(s.as_bytes());
    }
    pub fn read_string(&mut self) -> Result<String> {
        let len = self.read_varint()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes).map_err(|_| ProtoError::BadUtf8)
    }

    // ── UUID (128비트) ──
    pub fn write_uuid(&mut self, uuid: u128) {
        self.write_bytes(&uuid.to_be_bytes());
    }
    pub fn read_uuid(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.read_bytes(16)?.try_into().unwrap()))
    }

    // ── Position: x(26) z(26) y(12) 를 i64 로 패킹 ──
    pub fn write_position(&mut self, x: i32, y: i32, z: i32) {
        let v = (((x as i64) & 0x3FF_FFFF) << 38)
            | (((z as i64) & 0x3FF_FFFF) << 12)
            | ((y as i64) & 0xFFF);
        self.write_i64(v);
    }
    pub fn read_position(&mut self) -> Result<(i32, i32, i32)> {
        let v = self.read_i64()?;
        // i64 산술 시프트로 26/12/26 비트를 각각 부호 확장
        let x = (v >> 38) as i32;
        let y = ((v << 52) >> 52) as i32; // 하위 12비트
        let z = ((v << 26) >> 38) as i32; // 가운데 26비트
        Ok((x, y, z))
    }
}
