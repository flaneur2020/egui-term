use std::io::Write;

use base64::Engine as _;

const KITTY_CHUNK_SIZE: usize = 4096;

pub(crate) struct KittyFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub(crate) fn write_frame<W: Write>(
    writer: &mut W,
    frame: &KittyFrame,
    cols: u16,
    rows: u16,
) -> std::io::Result<()> {
    writer.write_all(b"\x1b[H")?;
    delete_all_images(writer)?;
    transmit_rgba(writer, frame, cols, rows)
}

fn delete_all_images<W: Write>(writer: &mut W) -> std::io::Result<()> {
    writer.write_all(b"\x1b_Ga=d,d=A\x1b\\")
}

fn transmit_rgba<W: Write>(
    writer: &mut W,
    frame: &KittyFrame,
    cols: u16,
    rows: u16,
) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(&frame.rgba);
    if encoded.is_empty() {
        return Ok(());
    }

    let bytes = encoded.as_bytes();
    let total_chunks = bytes.len().div_ceil(KITTY_CHUNK_SIZE);

    for (index, chunk) in bytes.chunks(KITTY_CHUNK_SIZE).enumerate() {
        let has_more = usize::from(index + 1 < total_chunks);

        if index == 0 {
            write!(
                writer,
                "\x1b_Ga=T,t=d,f=32,s={},v={},c={},r={},q=2,m={};",
                frame.width, frame.height, cols, rows, has_more
            )?;
        } else {
            write!(writer, "\x1b_Gm={};", has_more)?;
        }

        writer.write_all(chunk)?;
        writer.write_all(b"\x1b\\")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_kitty_escape_prefix() {
        let frame = KittyFrame {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };

        let mut out = Vec::new();
        write_frame(&mut out, &frame, 2, 2).unwrap();

        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\u{1b}_Ga=T,t=d,f=32"));
    }
}
