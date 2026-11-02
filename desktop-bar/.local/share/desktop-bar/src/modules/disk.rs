#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disk {
    pub free: f32,
    pub total: f32,
    pub used_percent: f32,
}
pub fn read() -> Option<Disk> {
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c"/home".as_ptr(), &mut s) } != 0 || s.f_blocks == 0 {
        return None;
    }
    let bytes = s.f_frsize as f64 / 1e9;
    Some(Disk {
        free: (s.f_bavail as f64 * bytes) as f32,
        total: (s.f_blocks as f64 * bytes) as f32,
        used_percent: 100.0 * (1.0 - s.f_bfree as f32 / s.f_blocks as f32),
    })
}
