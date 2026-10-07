//! Preserve the sender of catchable termination signals in the service journal.

fn append_number(buffer: &mut [u8], length: &mut usize, mut number: u32) {
    let mut digits = [0u8; 10];
    let mut first = digits.len();
    loop {
        first -= 1;
        digits[first] = b'0' + (number % 10) as u8;
        number /= 10;
        if number == 0 { break; }
    }
    let digits = &digits[first..];
    buffer[*length..*length + digits.len()].copy_from_slice(digits);
    *length += digits.len();
}

unsafe extern "C" fn log_signal(signal: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // Only stack storage and async-signal-safe system calls are used here.
    let mut message = [0u8; 256];
    let prefix = b"termination signal=";
    message[..prefix.len()].copy_from_slice(prefix);
    let mut length = prefix.len();
    append_number(&mut message, &mut length, signal as u32);
    if !info.is_null() {
        let pid = (*info).si_pid().max(0) as u32;
        let label = b" sender_pid=";
        message[length..length + label.len()].copy_from_slice(label);
        length += label.len();
        append_number(&mut message, &mut length, pid);
        let mut path = [0u8; 64];
        path[..6].copy_from_slice(b"/proc/");
        let mut path_length = 6;
        append_number(&mut path, &mut path_length, pid);
        path[path_length..path_length + 6].copy_from_slice(b"/comm\0");
        let descriptor = libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC);
        if descriptor >= 0 {
            let label = b" sender_comm=";
            message[length..length + label.len()].copy_from_slice(label);
            length += label.len();
            let read = libc::read(descriptor, message[length..].as_mut_ptr().cast(), 64);
            libc::close(descriptor);
            if read > 0 {
                length += read as usize;
                if message[length - 1] == b'\n' { length -= 1; }
            }
        }
    }
    message[length] = b'\n';
    libc::write(libc::STDERR_FILENO, message.as_ptr().cast(), length + 1);
    // SA_RESETHAND restored the default disposition. Preserve the original
    // signal exit status instead of turning a termination into a normal exit.
    libc::raise(signal);
}

pub fn install() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND;
        action.sa_sigaction = log_signal as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                eprintln!("cannot install signal diagnostics: {}", std::io::Error::last_os_error());
            }
        }
    }
}
