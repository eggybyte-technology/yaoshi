use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use yaoshi_common::{format_bytes_binary, format_percent_3};
use yaoshi_dashboard::{
    cpu_display_from_cpuinfo, disk_display, prepare_state_from_text,
    smbios_memory_summary_from_sysfs, ssh_ready_state, tcp_22_listening_from_proc_tables,
};
use yaoshi_screen::{
    DashboardCoreRow, DashboardDiskRow, DashboardNetworkRow, DashboardRouteRow, DashboardSnapshot,
    NegotiatedDisplay,
};

include!("sampler/state.rs");
include!("sampler/system.rs");
include!("sampler/storage.rs");
include!("sampler/network.rs");
include!("sampler/util.rs");
