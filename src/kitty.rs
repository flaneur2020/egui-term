use std::io::Write;

use base64::Engine as _;

const KITTY_CHUNK_SIZE: usize = 4096;

pub(crate) struct KittyFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Presents kitty frames with a two-image double buffer.
///
/// The new frame is uploaded and displayed before deleting the previous frame,
/// which reduces visible blanking between frames.
pub(crate) struct KittyPresenter {
    image_ids: [u32; 2],
    active: Option<usize>,
    z_index: i32,
}

impl Default for KittyPresenter {
    fn default() -> Self {
        Self {
            image_ids: [1, 2],
            active: None,
            z_index: 0,
        }
    }
}

impl KittyPresenter {
    pub(crate) fn present<W: Write>(
        &mut self,
        writer: &mut W,
        frame: &KittyFrame,
        cols: u16,
        rows: u16,
    ) -> std::io::Result<()> {
        let next = if self.active == Some(0) { 1 } else { 0 };
        let next_id = self.image_ids[next];
        let old_id = self.active.map(|index| self.image_ids[index]);

        self.z_index = self.z_index.wrapping_add(1);

        writer.write_all(b"\x1b[H")?;
        transmit_rgba(writer, frame, next_id, cols, rows, self.z_index)?;

        if let Some(old_id) = old_id {
            // Uppercase `I` frees data as well as placements, so the id can be
            // re-used for the next frame upload.
            write!(writer, "\x1b_Ga=d,d=I,i={}\x1b\\", old_id)?;
        }

        self.active = Some(next);
        Ok(())
    }

    pub(crate) fn clear<W: Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        for image_id in self.image_ids {
            write!(writer, "\x1b_Ga=d,d=I,i={}\x1b\\", image_id)?;
        }
        self.active = None;
        Ok(())
    }
}

fn transmit_rgba<W: Write>(
    writer: &mut W,
    frame: &KittyFrame,
    image_id: u32,
    cols: u16,
    rows: u16,
    z_index: i32,
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
                "\x1b_Ga=T,t=d,f=32,i={},p=1,C=1,s={},v={},c={},r={},z={},q=2,m={};",
                image_id, frame.width, frame.height, cols, rows, z_index, has_more
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
        let mut presenter = KittyPresenter::default();
        presenter.present(&mut out, &frame, 2, 2).unwrap();

        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\u{1b}_Ga=T,t=d,f=32,i=1"));
    }

    #[test]
    fn second_frame_deletes_previous_image() {
        let frame = KittyFrame {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };

        let mut out = Vec::new();
        let mut presenter = KittyPresenter::default();
        presenter.present(&mut out, &frame, 2, 2).unwrap();
        presenter.present(&mut out, &frame, 2, 2).unwrap();

        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\u{1b}_Ga=T,t=d,f=32,i=2"));
        assert!(text.contains("\u{1b}_Ga=d,d=I,i=1"));
    }
}
