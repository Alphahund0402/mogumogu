//! Export approved artwork without redrawing it. Uses Slint's cached renderer.
use resvg::tiny_skia::{FilterQuality, IntRect, Pixmap, PixmapPaint, Transform};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn crop(sheet: &Pixmap, rect: (i32, i32, u32, u32), output: &Path) -> Result<Pixmap> {
    let (x, y, width, height) = rect;
    let image = sheet
        .clone_rect(IntRect::from_xywh(x, y, width, height).ok_or("Invalid crop")?)
        .ok_or("Crop outside source")?;
    image.save_png(output)?;
    // Re-encoding must not change the selected source pixels.
    assert_eq!(Pixmap::decode_png(&std::fs::read(output)?)?.data(), image.data());
    Ok(image)
}

fn resize(image: &Pixmap, size: u32) -> Result<Pixmap> {
    let mut output = Pixmap::new(size, size).ok_or("Invalid icon size")?;
    let scale = size as f32 / image.width() as f32;
    let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
    output.draw_pixmap(0, 0, image.as_ref(), &paint, Transform::from_scale(scale, scale), None);
    Ok(output)
}

fn main() -> Result<()> {
    let root = std::env::args().nth(1).ok_or("Pass the project root")?;
    let root = Path::new(&root);
    let sheet = Pixmap::decode_png(&std::fs::read(root.join("design/brand/source/mochi-cloud.png"))?)?;
    assert_eq!((sheet.width(), sheet.height()), (1536, 1024));
    let output = root.join("assets/brand");
    std::fs::create_dir_all(&output)?;
    let icon = crop(&sheet, (732, 783, 168, 168), &output.join("icon.png"))?;
    crop(&sheet, (12, 234, 494, 468), &output.join("mascot.png"))?;
    crop(&sheet, (61, 165, 224, 50), &output.join("wordmark.png"))?;
    // Equal canvases keep the expression switch inside the same sidebar slot.
    crop(&sheet, (10, 772, 224, 184), &output.join("mascot-neutral.png"))?;
    crop(&sheet, (225, 772, 224, 184), &output.join("mascot-happy.png"))?;
    crop(&sheet, (449, 772, 224, 184), &output.join("mascot-focused.png"))?;
    resize(&icon, 64)?.save_png(root.join("assets/tray.png"))?;

    // PNG-backed ICO frames are supported on the project's Windows 10/11 target.
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let frames =
        sizes.iter().map(|&size| resize(&icon, size)?.encode_png().map_err(Into::into)).collect::<Result<Vec<_>>>()?;
    let mut ico = Vec::from([0, 0, 1, 0]);
    ico.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, frame) in sizes.iter().zip(&frames) {
        ico.extend([size as u8, size as u8, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((frame.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += frame.len() as u32;
    }
    for frame in frames {
        ico.extend(frame);
    }
    std::fs::write(output.join("mogumogu.ico"), ico)?;
    println!("Exported exact Mochi artwork and expressions, tray PNG and nine Windows ICO sizes.");
    Ok(())
}
