use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn until_minute() -> Duration {
    Duration::from_secs(60 - now() % 60)
}
pub fn format(pattern: &str) -> String {
    let time = now() as libc::time_t;
    let mut tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&time, &mut tm);
    }
    let Ok(pattern) = std::ffi::CString::new(pattern) else {
        return String::new();
    };
    let mut buffer = [0_u8; 128];
    let length = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            pattern.as_ptr(),
            &tm,
        )
    };
    String::from_utf8_lossy(&buffer[..length]).into_owned()
}
pub fn calendar(offset: i32) -> (String, Vec<(String, bool)>) {
    let time = now() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&time, &mut tm);
    }
    let today = tm.tm_mday;
    tm.tm_mday = 1;
    tm.tm_mon += offset;
    tm.tm_hour = 12;
    tm.tm_isdst = -1;
    unsafe {
        libc::mktime(&mut tm);
    }
    let title = format!(
        "{} {}",
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December"
        ][tm.tm_mon as usize],
        tm.tm_year + 1900
    );
    let mut cells = vec![(String::new(), false); ((tm.tm_wday + 6) % 7) as usize];
    let month = tm.tm_mon;
    for day in 1..=31 {
        tm.tm_mday = day;
        tm.tm_isdst = -1;
        unsafe {
            libc::mktime(&mut tm);
        }
        if tm.tm_mon != month {
            break;
        }
        cells.push((day.to_string(), offset == 0 && day == today));
    }
    (title, cells)
}
