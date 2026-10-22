use anyhow::{Result, ensure};
use gtk::cairo::{Context, Surface};
use std::ffi::{CString, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// Keep the GTK feature set small while using Cairo's stable PDF constructor.
#[link(name = "cairo")]
unsafe extern "C" {
    fn cairo_pdf_surface_create(
        filename: *const c_char,
        width: f64,
        height: f64,
    ) -> *mut gtk::cairo::ffi::cairo_surface_t;
}

pub fn write(scene: &crate::render::Scene, path: &Path, width: f64, height: f64) -> Result<()> {
    let filename = CString::new(path.as_os_str().as_bytes())?;
    // Cairo returns a newly owned surface. from_raw_full assumes ownership
    // exactly once and checks its status before a Context can use it.
    let scale = 72.0 / 100.0;
    let pointer =
        unsafe { cairo_pdf_surface_create(filename.as_ptr(), width * scale, height * scale) };
    ensure!(!pointer.is_null(), "Could not allocate PDF surface");
    let surface = unsafe { Surface::from_raw_full(pointer)? };
    {
        let context = Context::new(&surface)?;
        context.scale(scale, scale);
        scene.draw(&context)?;
        context.show_page()?;
    }
    surface.finish();
    surface.status()?;
    Ok(())
}
