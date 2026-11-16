//! Embeds the orDrumbox icon: in `ordrumbox.exe` (Windows builds only), and
//! as raw pixels for the window icon (`window_icon.rs`).

use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;

const ICON_PNG: &str = "../../installer/ordrumbox-setup.png";
const ICON_SIZE: u32 = 256;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=../../installer/ordrumbox.ico");
    println!("cargo:rerun-if-changed={ICON_PNG}");
    window_icon_pixels()?;
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../installer/ordrumbox.ico");
        res.compile()?;
    }
    Ok(())
}

/// Decodes the icon PNG (8-bit RGBA) into `$OUT_DIR/window_icon.rgba`, which `window_icon.rs` includes.
fn window_icon_pixels() -> Result<(), Box<dyn Error>> {
    let mut reader = png::Decoder::new(BufReader::new(File::open(ICON_PNG)?)).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("the icon PNG is too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || (info.width, info.height) != (ICON_SIZE, ICON_SIZE)
    {
        return Err(format!("{ICON_PNG} must be {ICON_SIZE} x {ICON_SIZE}, 8-bit RGBA").into());
    }
    pixels.truncate(info.buffer_size());
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("window_icon.rgba"), pixels)?;
    Ok(())
}
