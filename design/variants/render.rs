//! Preview-only SVG renderer, using dependencies already built for Slint.
use resvg::{tiny_skia, usvg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = usvg::Options::default();
    let fonts =
        std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    for name in ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"] {
        options.fontdb_mut().load_font_data(std::fs::read(fonts.join(name))?);
    }
    for path in std::env::args().skip(1) {
        let source = std::fs::read_to_string(&path)?;
        let tree = usvg::Tree::from_str(&source, &options)?;
        let size = tree.size().to_int_size();
        let mut pixels = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("Invalid size")?;
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixels.as_mut());
        let output = std::path::Path::new(&path).with_extension("png");
        pixels.save_png(&output)?;
        println!("{}", output.display());
    }
    Ok(())
}
