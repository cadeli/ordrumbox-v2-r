// Embeds the orDrumbox icon in `ordrumbox.exe` (Windows builds only).

fn main() {
    println!("cargo:rerun-if-changed=../../installer/ordrumbox.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../installer/ordrumbox.ico");
        res.compile().expect("embed the icon in ordrumbox.exe");
    }
}
