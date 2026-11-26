//! Packet framing: length prefix (VarInt) + optional zlib compression.
//!
//! Before compression (before Set Compression): `VarInt(len) | VarInt(id) | body`
//! After compression (threshold set):
//!   `VarInt(frameLen) | VarInt(dataLen) | (dataLen==0 ? raw : zlib(raw))`
//!   where raw = `VarInt(id) | body` and dataLen is the decompressed length (0 = not compressed).

use crate::buf::PacketBuf;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{self, Read, Write};

/// Read one VarInt byte by byte from a Read (used for the frame length).
pub fn read_varint_from<R: Read>(reader: &mut R) -> io::Result<i32> {
    let mut result: u32 = 0;
    let mut shift = 0;
    loop {
        let mut b = [0u8; 1];
        reader.read_exact(&mut b)?;
        result |= ((b[0] & 0x7F) as u32) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too long"));
        }
    }
    Ok(result as i32)
}

/// Encode one packet to wire bytes. threshold = None means no compression.
pub fn encode(id: i32, body: &[u8], threshold: Option<usize>) -> Vec<u8> {
    let mut packet = PacketBuf::new();
    packet.write_varint(id);
    packet.write_bytes(body);
    let packet = packet.into_vec();

    let mut out = PacketBuf::new();
    match threshold {
        None => {
            out.write_varint(packet.len() as i32);
            out.write_bytes(&packet);
        }
        Some(th) => {
            let mut frame = PacketBuf::new();
            if packet.len() >= th {
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(&packet).unwrap();
                let compressed = enc.finish().unwrap();
                frame.write_varint(packet.len() as i32); // raw length
                frame.write_bytes(&compressed);
            } else {
                frame.write_varint(0); // marks "not compressed"
                frame.write_bytes(&packet);
            }
            let frame = frame.into_vec();
            out.write_varint(frame.len() as i32);
            out.write_bytes(&frame);
        }
    }
    out.into_vec()
}

/// Read one frame from a Read and return (packet id, body buffer).
pub fn read_frame<R: Read>(reader: &mut R, threshold: Option<usize>) -> io::Result<(i32, PacketBuf)> {
    let frame_len = read_varint_from(reader)? as usize;
    let mut frame = vec![0u8; frame_len];
    reader.read_exact(&mut frame)?;

    let packet_bytes = match threshold {
        None => frame,
        Some(_) => {
            let mut fb = PacketBuf::from_vec(frame);
            let data_len = fb
                .read_varint()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                as usize;
            let rest = fb.read_rest();
            if data_len == 0 {
                rest // not compressed
            } else {
                let mut dec = ZlibDecoder::new(&rest[..]);
                let mut out = Vec::with_capacity(data_len);
                dec.read_to_end(&mut out)?;
                out
            }
        }
    };

    let mut pb = PacketBuf::from_vec(packet_bytes);
    let id = pb
        .read_varint()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok((id, pb))
}

/// Convenience: encode and write straight to a Write.
pub fn write_frame<W: Write>(writer: &mut W, id: i32, body: &[u8], threshold: Option<usize>) -> io::Result<()> {
    writer.write_all(&encode(id, body, threshold))
}
