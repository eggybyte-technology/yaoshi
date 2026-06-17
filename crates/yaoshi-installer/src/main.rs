use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::FileExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nix::libc;
use nix::mount::{MsFlags, mount};
use nix::poll::{PollFd, PollFlags, poll};
use nix::sys::reboot::{RebootMode, reboot};
use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use nix::unistd::{Pid, getpid, sync};
use yaoshi_common::markers;
use yaoshi_common::*;
use yaoshi_installer::{FailureReason, InstallerPhase};

static RESIZE_PENDING: AtomicBool = AtomicBool::new(false);

const MODULE_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const BLOCKING_STOP_POWEROFF_TIMEOUT: Duration = Duration::from_secs(15);
const SOURCE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const DISK_CLASSIFY_TIMEOUT: Duration = Duration::from_secs(2);
const SERIAL_MARK_TIMEOUT: Duration = Duration::from_secs(1);
const SERIAL_FRAME_LINE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
struct DiskInspectError(String);

impl std::fmt::Display for DiskInspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

struct DiskRecord {
    disk: KernelDiskRef,
    read_error: bool,
}

struct OpenedTarget {
    target: TargetDiskCandidate,
    file: File,
}

struct WriteResult {
    target_dev_path: PathBuf,
    target_stable_id: Option<String>,
    bytes_written: u64,
}

include!("runtime/bootstrap.rs");
include!("runtime/select.rs");
include!("runtime/write.rs");
include!("runtime/screen.rs");
include!("runtime/serial.rs");
#[cfg(test)]
include!("runtime/write_tests.rs");
