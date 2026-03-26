use std::io::Write;

use base64::Engine as _;

const KITTY_CHUNK_SIZE: usize = 4096;
const DIRTY_RECT_FULL_REPAINT_THRESHOLD: f32 = 0.45;

pub(crate) struct KittyFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirtyRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl DirtyRect {
    fn pixel_count(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

struct PreviousFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Presents kitty frames with a two-image double buffer.
///
/// For small updates it performs dirty-rect detection and patches the current
/// image using `a=f` frame edits, avoiding full-screen re-upload.
pub(crate) struct KittyPresenter {
    image_ids: [u32; 2],
    active: Option<usize>,
    z_index: i32,
    previous: Option<PreviousFrame>,
    disable_dirty_rect: bool,
}

impl Default for KittyPresenter {
    fn default() -> Self {
        let disable_dirty_rect = std::env::var("EGUI_TERM_DISABLE_DIRTY_RECT")
            .map(|value| value == "1")
            .unwrap_or(false);
        Self {
            image_ids: [1, 2],
            active: None,
            z_index: 0,
            previous: None,
            disable_dirty_rect,
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
        if self.disable_dirty_rect {
            return self.full_repaint(writer, frame, cols, rows);
        }

        if let (Some(active_index), Some(previous)) = (self.active, self.previous.as_mut()) {
            let same_geometry = previous.width == frame.width
                && previous.height == frame.height
                && previous.rgba.len() == frame.rgba.len();
            if same_geometry {
                if let Some(dirty) = dirty_rect(previous, frame) {
                    let total_pixels = frame.width as u64 * frame.height as u64;
                    let dirty_ratio = dirty.pixel_count() as f32 / total_pixels.max(1) as f32;

                    if dirty_ratio <= DIRTY_RECT_FULL_REPAINT_THRESHOLD {
                        let patch = crop_rgba(frame, dirty);
                        transmit_frame_patch(writer, self.image_ids[active_index], dirty, &patch)?;
                        // Kitty applies animation frame updates after selecting the target frame.
                        write!(
                            writer,
                            "\x1b_Ga=a,i={},c=1,q=2\x1b\\",
                            self.image_ids[active_index]
                        )?;
                        previous.rgba.clone_from_slice(&frame.rgba);
                        return Ok(());
                    }
                } else {
                    // Nothing changed.
                    return Ok(());
                }
            }
        }

        self.full_repaint(writer, frame, cols, rows)
    }

    pub(crate) fn clear<W: Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        for image_id in self.image_ids {
            write!(writer, "\x1b_Ga=d,d=I,i={}\x1b\\", image_id)?;
        }
        self.active = None;
        self.previous = None;
        Ok(())
    }

    fn full_repaint<W: Write>(
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
        transmit_full_image(writer, frame, next_id, cols, rows, self.z_index)?;

        if let Some(old_id) = old_id {
            // Uppercase `I` frees data as well as placements, so the id can be
            // re-used for the next frame upload.
            write!(writer, "\x1b_Ga=d,d=I,i={}\x1b\\", old_id)?;
        }

        self.active = Some(next);
        self.previous = Some(PreviousFrame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.clone(),
        });
        Ok(())
    }
}

fn dirty_rect(previous: &PreviousFrame, current: &KittyFrame) -> Option<DirtyRect> {
    dirty_rect_rgba(&previous.rgba, &current.rgba, current.width, current.height)
}

fn dirty_rect_rgba(old: &[u8], new: &[u8], width: u32, height: u32) -> Option<DirtyRect> {
    let stride = width as usize * 4;
    if stride == 0 || height == 0 || old.len() != new.len() {
        return None;
    }

    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    let mut changed = false;

    for y in 0..height as usize {
        let row_start = y * stride;
        let row_end = row_start + stride;
        let old_row = &old[row_start..row_end];
        let new_row = &new[row_start..row_end];

        if old_row == new_row {
            continue;
        }

        changed = true;
        let y_u32 = y as u32;
        min_y = min_y.min(y_u32);
        max_y = max_y.max(y_u32);

        for x in 0..width as usize {
            let offset = x * 4;
            if old_row[offset..offset + 4] != new_row[offset..offset + 4] {
                let x_u32 = x as u32;
                min_x = min_x.min(x_u32);
                max_x = max_x.max(x_u32);
            }
        }
    }

    if !changed {
        return None;
    }

    Some(DirtyRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    })
}

fn crop_rgba(frame: &KittyFrame, rect: DirtyRect) -> Vec<u8> {
    let bytes_per_pixel = 4_usize;
    let src_stride = frame.width as usize * bytes_per_pixel;
    let row_bytes = rect.width as usize * bytes_per_pixel;

    let mut out = Vec::with_capacity((rect.width * rect.height * 4) as usize);
    for row in rect.y..(rect.y + rect.height) {
        let src_start = row as usize * src_stride + rect.x as usize * bytes_per_pixel;
        out.extend_from_slice(&frame.rgba[src_start..src_start + row_bytes]);
    }
    out
}

fn transmit_full_image<W: Write>(
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

fn transmit_frame_patch<W: Write>(
    writer: &mut W,
    image_id: u32,
    rect: DirtyRect,
    rgba: &[u8],
) -> std::io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(rgba);
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
                "\x1b_Ga=f,t=d,f=32,i={},r=1,x={},y={},s={},v={},q=2,m={};",
                image_id, rect.x, rect.y, rect.width, rect.height, has_more
            )?;
        } else {
            // `a=f` must be present on subsequent chunks for animation-frame transfers.
            write!(writer, "\x1b_Ga=f,r=1,m={};", has_more)?;
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
    fn detects_single_pixel_dirty_rect() {
        let width = 4;
        let height = 4;
        let mut old = vec![0_u8; (width * height * 4) as usize];
        let mut new = old.clone();

        let idx = ((1 * width + 2) * 4) as usize;
        new[idx] = 255;
        old[idx + 1] = 1;

        let rect = dirty_rect_rgba(&old, &new, width, height).unwrap();
        assert_eq!(
            rect,
            DirtyRect {
                x: 2,
                y: 1,
                width: 1,
                height: 1,
            }
        );
    }

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
    fn second_frame_uses_frame_patch() {
        let frame1 = KittyFrame {
            width: 4,
            height: 4,
            rgba: vec![0; 64],
        };

        let mut frame2 = KittyFrame {
            width: 4,
            height: 4,
            rgba: vec![0; 64],
        };
        let idx = ((1 * 4 + 2) * 4) as usize;
        frame2.rgba[idx] = 255;

        let mut out = Vec::new();
        let mut presenter = KittyPresenter::default();
        presenter.present(&mut out, &frame1, 4, 4).unwrap();

        let start = out.len();
        presenter.present(&mut out, &frame2, 4, 4).unwrap();

        let text = String::from_utf8_lossy(&out[start..]);
        assert!(text.contains("\u{1b}_Ga=f,t=d,f=32,i=1,r=1,x=2,y=1,s=1,v=1"));
        assert!(text.contains("\u{1b}_Ga=a,i=1,c=1,q=2"));
    }
}
