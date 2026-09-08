//! Independent cursor channel so the tablet can paint the pointer locally.
//!
//! The encoded desktop still carries game-drawn cursors. Hardware / system
//! pointers go through this side channel instead of waiting on encode + decode.

pub const MSG_CURSOR: u8 = 8;
pub const FLAG_VISIBLE: u8 = 1 << 0;
pub const FLAG_HAS_SHAPE: u8 = 1 << 1;
pub const HEADER_LEN: usize = 14;
pub const MAX_CURSOR_EDGE: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorPacket {
    pub visible: bool,
    pub x: i16,
    pub y: i16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub width: u32,
    pub height: u32,
    /// BGRA8888, top-down. `None` means keep the last shape.
    pub bgra: Option<Vec<u8>>,
}

pub fn encode_cursor(pkt: &CursorPacket) -> Vec<u8> {
    let mut flags = 0u8;
    if pkt.visible {
        flags |= FLAG_VISIBLE;
    }
    let source_row_bytes = (pkt.width as usize).checked_mul(4);
    let source_len = source_row_bytes.and_then(|row| row.checked_mul(pkt.height as usize));
    let shape = pkt.bgra.as_ref().filter(|bytes| {
        pkt.width > 0
            && pkt.height > 0
            && source_len.is_some_and(|expected| bytes.len() >= expected)
    });
    if shape.is_some() {
        flags |= FLAG_HAS_SHAPE;
    }
    let w = pkt.width.min(MAX_CURSOR_EDGE);
    let h = pkt.height.min(MAX_CURSOR_EDGE);
    let mut out = Vec::with_capacity(HEADER_LEN + shape.map(|b| b.len()).unwrap_or(0));
    out.push(flags);
    out.push(0);
    out.extend_from_slice(&pkt.x.to_be_bytes());
    out.extend_from_slice(&pkt.y.to_be_bytes());
    out.extend_from_slice(&pkt.hotspot_x.to_be_bytes());
    out.extend_from_slice(&pkt.hotspot_y.to_be_bytes());
    out.extend_from_slice(&(w as u16).to_be_bytes());
    out.extend_from_slice(&(h as u16).to_be_bytes());
    if let Some(bgra) = shape {
        let source_row_bytes = source_row_bytes.expect("validated cursor row size");
        let cropped_row_bytes = w as usize * 4;
        for row in 0..h as usize {
            let start = row * source_row_bytes;
            out.extend_from_slice(&bgra[start..start + cropped_row_bytes]);
        }
    }
    out
}

pub fn decode_cursor(buf: &[u8]) -> Option<CursorPacket> {
    if buf.len() < HEADER_LEN {
        return None;
    }
    let flags = buf[0];
    let x = i16::from_be_bytes([buf[2], buf[3]]);
    let y = i16::from_be_bytes([buf[4], buf[5]]);
    let hotspot_x = u16::from_be_bytes([buf[6], buf[7]]);
    let hotspot_y = u16::from_be_bytes([buf[8], buf[9]]);
    let width = u16::from_be_bytes([buf[10], buf[11]]) as u32;
    let height = u16::from_be_bytes([buf[12], buf[13]]) as u32;
    if width > MAX_CURSOR_EDGE || height > MAX_CURSOR_EDGE {
        return None;
    }
    let has_shape = flags & FLAG_HAS_SHAPE != 0;
    let bgra = if has_shape {
        let expect = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if expect == 0 || buf.len() < HEADER_LEN + expect {
            return None;
        }
        Some(buf[HEADER_LEN..HEADER_LEN + expect].to_vec())
    } else {
        None
    };
    Some(CursorPacket {
        visible: flags & FLAG_VISIBLE != 0,
        x,
        y,
        hotspot_x,
        hotspot_y,
        width,
        height,
        bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hidden_cursor() {
        let bytes = encode_cursor(&CursorPacket {
            visible: false,
            x: 0,
            y: 0,
            hotspot_x: 0,
            hotspot_y: 0,
            width: 0,
            height: 0,
            bgra: None,
        });
        let pkt = decode_cursor(&bytes).unwrap();
        assert!(!pkt.visible);
        assert!(pkt.bgra.is_none());
        assert_eq!(bytes.len(), HEADER_LEN);
    }

    #[test]
    fn roundtrip_move_without_new_shape() {
        let bytes = encode_cursor(&CursorPacket {
            visible: true,
            x: 1920,
            y: -12,
            hotspot_x: 0,
            hotspot_y: 0,
            width: 32,
            height: 32,
            bgra: None,
        });
        let pkt = decode_cursor(&bytes).unwrap();
        assert!(pkt.visible);
        assert_eq!(pkt.x, 1920);
        assert_eq!(pkt.y, -12);
        assert!(pkt.bgra.is_none());
    }

    #[test]
    fn roundtrip_shape_bgra() {
        let mut bgra = vec![0u8; 4 * 2 * 2];
        bgra[0] = 10;
        bgra[3] = 255;
        let bytes = encode_cursor(&CursorPacket {
            visible: true,
            x: 40,
            y: 80,
            hotspot_x: 1,
            hotspot_y: 2,
            width: 2,
            height: 2,
            bgra: Some(bgra.clone()),
        });
        let pkt = decode_cursor(&bytes).unwrap();
        assert_eq!(pkt.hotspot_x, 1);
        assert_eq!(pkt.hotspot_y, 2);
        assert_eq!(pkt.bgra.unwrap(), bgra);
    }

    #[test]
    fn truncated_payload_is_rejected() {
        assert!(decode_cursor(&[1, 0, 0]).is_none());
        let mut bytes = encode_cursor(&CursorPacket {
            visible: true,
            x: 0,
            y: 0,
            hotspot_x: 0,
            hotspot_y: 0,
            width: 2,
            height: 2,
            bgra: Some(vec![0; 16]),
        });
        bytes.pop();
        assert!(decode_cursor(&bytes).is_none());
    }

    #[test]
    fn invalid_shape_is_not_advertised() {
        let bytes = encode_cursor(&CursorPacket {
            visible: true,
            x: 0,
            y: 0,
            hotspot_x: 0,
            hotspot_y: 0,
            width: 16,
            height: 16,
            bgra: Some(vec![0; 8]),
        });

        assert_eq!(bytes[0] & FLAG_HAS_SHAPE, 0);
        assert!(decode_cursor(&bytes).is_some());
    }

    #[test]
    fn oversized_shape_is_cropped_by_rows() {
        let width = MAX_CURSOR_EDGE + 1;
        let height = 2;
        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        for y in 0..height as usize {
            for x in 0..width as usize {
                bgra[(y * width as usize + x) * 4] = y as u8 + 1;
            }
        }

        let bytes = encode_cursor(&CursorPacket {
            visible: true,
            x: 0,
            y: 0,
            hotspot_x: 0,
            hotspot_y: 0,
            width,
            height,
            bgra: Some(bgra),
        });
        let pkt = decode_cursor(&bytes).expect("cropped packet should remain valid");
        let shape = pkt.bgra.expect("shape should be present");

        assert_eq!(pkt.width, MAX_CURSOR_EDGE);
        assert_eq!(shape[0], 1);
        assert_eq!(shape[MAX_CURSOR_EDGE as usize * 4], 2);
    }

    #[test]
    fn oversized_wire_shape_is_rejected() {
        let mut bytes = vec![0u8; HEADER_LEN + (MAX_CURSOR_EDGE as usize + 1) * 4];
        bytes[0] = FLAG_VISIBLE | FLAG_HAS_SHAPE;
        bytes[10..12].copy_from_slice(&((MAX_CURSOR_EDGE + 1) as u16).to_be_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_be_bytes());

        assert!(decode_cursor(&bytes).is_none());
    }
}
