use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn request_stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
}

pub fn install() {
    unsafe {
        let handler = request_stop as *const () as usize;
        signal(2, handler); // SIGINT
        signal(15, handler); // SIGTERM
    }
}

pub fn should_stop() -> bool {
    STOP.load(Ordering::Relaxed)
}

pub fn ignore_in_child(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            signal(2, 1); // SIGINT
            signal(15, 1); // SIGTERM
            Ok(())
        });
    }
}
