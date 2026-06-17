use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::style::{
    Attribute, Color as CrosstermColor, Print, ResetColor, SetAttribute, SetForegroundColor,
};
use crossterm::terminal::{
    Clear, ClearType, DisableLineWrap, EnableLineWrap, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, QueueableCommand};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use yaoshi_common::{
    CandidateStatus, ExistingPartitionTable, TargetDiskCandidate, VERSION, WriteTask,
    format_byte_rate_binary, format_capacity_binary, format_duration_seconds_3,
    format_exact_byte_count, format_percent,
};

include!("render/display.rs");
include!("render/models.rs");
include!("render/installer.rs");
include!("render/dashboard.rs");
include!("render/buffer.rs");
include!("render/terminal.rs");
include!("render/fixtures.rs");
include!("render/tests.rs");
