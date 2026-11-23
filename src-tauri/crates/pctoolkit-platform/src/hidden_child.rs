//! Containment for hidden CLI child processes (dev tools, winget, disk tools).
//!
//! A child killed on timeout / cancel can leave grandchildren that inherited its stdout pipe; a
//! reader thread joined without a bound then blocks forever. Children are therefore placed in a
//! Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (the whole tree is terminated on timeout /
//! cancel, and if this process dies), and output readers are joined with a deadline: a reader
//! still blocked after it is detached and the output collected so far is used.

use std::io::Read;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long to wait for output readers after the child exited or was killed.
pub(crate) const READER_GRACE: Duration = Duration::from_secs(2);

/// Job Object holding a child process tree.
///
/// The child is assigned right after spawn, so a grandchild started in that instant can escape
/// the job; the bounded reader join still keeps the caller from hanging in that case.
pub(crate) struct ChildJob {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

impl ChildJob {
    /// Create a kill-on-close job and assign `child` to it. `None` when the job cannot be
    /// created or assigned (the caller still kills the direct child).
    pub(crate) fn attach(child: &Child) -> Option<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::core::PCWSTR;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};

            let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.ok()?;
            let job = Self { handle };
            if !job.set_kill_on_close(true) {
                return None;
            }
            let process = HANDLE(child.as_raw_handle());
            unsafe { AssignProcessToJobObject(job.handle, process) }.ok()?;
            Some(job)
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            None
        }
    }

    #[cfg(windows)]
    fn set_kill_on_close(&self, enabled: bool) -> bool {
        use windows::Win32::System::JobObjects::{
            JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if enabled {
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        unsafe {
            SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .is_ok()
    }

    /// Terminate every process in the job (timeout / cancel).
    pub(crate) fn terminate(&self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::JobObjects::TerminateJobObject(self.handle, 1);
        }
    }

    /// The child finished normally: close the job without killing anything it legitimately left
    /// running (for example an application an installer started).
    pub(crate) fn release(self) {
        #[cfg(windows)]
        self.set_kill_on_close(false);
    }
}

impl Drop for ChildJob {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Drains a child pipe on its own thread into a capped shared buffer.
pub(crate) struct CappedReader {
    buffer: Arc<Mutex<Vec<u8>>>,
    handle: thread::JoinHandle<()>,
}

impl CappedReader {
    /// Read `source` until EOF, keeping at most `cap` bytes (it keeps draining past the cap so
    /// the child never blocks on a full pipe). `on_chunk` sees every chunk as it arrives.
    pub(crate) fn spawn<R, F>(mut source: R, cap: usize, mut on_chunk: F) -> Self
    where
        R: Read + Send + 'static,
        F: FnMut(&[u8]) + Send + 'static,
    {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&buffer);
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match source.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        on_chunk(&buf[..n]);
                        let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let room = cap.saturating_sub(kept.len());
                        kept.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
        });
        Self { buffer, handle }
    }

    /// Wait up to `grace` for EOF, then return what was read. A reader still blocked (a
    /// grandchild outside the job holds the pipe) is detached; it ends when the pipe closes.
    pub(crate) fn finish(self, grace: Duration) -> Vec<u8> {
        let deadline = Instant::now() + grace;
        while !self.handle.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if self.handle.is_finished() {
            let _ = self.handle.join();
        }
        let kept = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        kept.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that never reaches EOF, like a pipe a detached grandchild keeps open.
    struct Endless {
        sent: bool,
    }

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.sent {
                self.sent = true;
                buf[..5].copy_from_slice(b"hello");
                return Ok(5);
            }
            thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    #[test]
    fn finish_does_not_hang_on_a_pipe_that_never_closes() {
        let reader = CappedReader::spawn(Endless { sent: false }, 1024, |_| {});
        let started = Instant::now();
        thread::sleep(Duration::from_millis(50));
        let out = reader.finish(Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(out, b"hello");
    }

    #[test]
    fn output_is_capped_but_fully_drained() {
        let data = vec![b'x'; 100_000];
        let seen = Arc::new(Mutex::new(0usize));
        let counter = Arc::clone(&seen);
        let reader = CappedReader::spawn(std::io::Cursor::new(data), 10, move |chunk| {
            *counter.lock().unwrap() += chunk.len();
        });
        assert_eq!(reader.finish(Duration::from_secs(5)).len(), 10);
        assert_eq!(*seen.lock().unwrap(), 100_000);
    }

    #[cfg(windows)]
    #[test]
    fn terminating_the_job_kills_grandchildren_holding_the_pipe() {
        use std::process::{Command, Stdio};
        // cmd starts a long-lived grandchild that inherits stdout, then waits for it.
        let mut child = Command::new(crate::registry::system32_exe("cmd.exe"))
            .args(["/C", "ping -n 30 127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let job = ChildJob::attach(&child).expect("job attach");
        let reader = CappedReader::spawn(child.stdout.take().unwrap(), 1 << 20, |_| {});
        thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        job.terminate();
        let _ = child.wait();
        let _ = reader.finish(READER_GRACE);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
