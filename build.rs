extern crate winres;

fn main() {
    slint_build::compile("ui/ui.slint").expect("error compiling slint files");

    #[cfg(target_os = "windows")]
    let mut res = winres::WindowsResource::new();
    res.set_icon("og3-pack-updater-icon.ico");
    res.compile().unwrap();
}
