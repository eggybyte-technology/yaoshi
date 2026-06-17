#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticStyle {
    Canvas,
    Plain,
    Header,
    Section,
    Muted,
    Info,
    Focus,
    Action,
    ActionFocus,
    DestructiveFocus,
    Warning,
    Success,
    Disabled,
    Critical,
    GaugeFill,
    GaugeSuccess,
    GaugeWarning,
    GaugeCritical,
    GaugeRest,
}

pub fn semantic_style_definition(style: SemanticStyle) -> (Color, Color, Modifier) {
    use SemanticStyle::*;
    match style {
        Canvas => (Color::Reset, Color::Reset, Modifier::empty()),
        Plain | Info | Action => (Color::White, Color::Reset, Modifier::empty()),
        Header => (Color::White, Color::Reset, Modifier::BOLD),
        Section | Focus | ActionFocus => (Color::LightCyan, Color::Reset, Modifier::BOLD),
        Muted => (Color::DarkGray, Color::Reset, Modifier::empty()),
        DestructiveFocus | Critical => (Color::LightRed, Color::Reset, Modifier::BOLD),
        Warning => (Color::Yellow, Color::Reset, Modifier::BOLD),
        Success => (Color::Green, Color::Reset, Modifier::BOLD),
        Disabled => (Color::DarkGray, Color::Reset, Modifier::empty()),
        GaugeFill => (Color::Cyan, Color::Reset, Modifier::empty()),
        GaugeSuccess => (Color::Green, Color::Reset, Modifier::empty()),
        GaugeWarning => (Color::Yellow, Color::Reset, Modifier::empty()),
        GaugeCritical => (Color::LightRed, Color::Reset, Modifier::empty()),
        GaugeRest => (Color::DarkGray, Color::Reset, Modifier::empty()),
    }
}

fn rat_style(style: SemanticStyle) -> Style {
    let (fg, _bg, modifiers) = semantic_style_definition(style);
    Style::default()
        .fg(fg)
        .underline_color(semantic_style_marker(style))
        .add_modifier(modifiers)
}

fn semantic_style_marker(style: SemanticStyle) -> Color {
    Color::Indexed(match style {
        SemanticStyle::Canvas => 16,
        SemanticStyle::Plain => 17,
        SemanticStyle::Header => 18,
        SemanticStyle::Section => 19,
        SemanticStyle::Muted => 20,
        SemanticStyle::Info => 21,
        SemanticStyle::Focus => 22,
        SemanticStyle::Action => 23,
        SemanticStyle::ActionFocus => 24,
        SemanticStyle::DestructiveFocus => 25,
        SemanticStyle::Warning => 26,
        SemanticStyle::Success => 27,
        SemanticStyle::Disabled => 28,
        SemanticStyle::Critical => 29,
        SemanticStyle::GaugeFill => 30,
        SemanticStyle::GaugeSuccess => 31,
        SemanticStyle::GaugeWarning => 32,
        SemanticStyle::GaugeCritical => 33,
        SemanticStyle::GaugeRest => 34,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFocus {
    ConfirmTarget,
    Refresh,
    PowerOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallFocus {
    Back,
    Erase,
    Destructive,
    PowerOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOperation {
    Install,
    Erase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoneFocus {
    ChooseTarget,
    Reboot,
    PowerOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureWriteState {
    NoTargetWrites,
    TargetWriteStarted,
}

impl FailureWriteState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoTargetWrites => "no-target-writes",
            Self::TargetWriteStarted => "target-write-started",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    InstallationMediaNotFound,
    InstallationMediaAmbiguous,
    PayloadValidationFailed,
    DiskInspectionFailed,
    SelectedDiskChanged,
    TargetOpenFailed,
    TargetWriteFailed,
    ModuleLoadFailed,
    InternalError,
}

impl FailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InstallationMediaNotFound => "installation-media-not-found",
            Self::InstallationMediaAmbiguous => "installation-media-ambiguous",
            Self::PayloadValidationFailed => "payload-validation-failed",
            Self::DiskInspectionFailed => "disk-inspection-failed",
            Self::SelectedDiskChanged => "selected-disk-changed",
            Self::TargetOpenFailed => "target-open-failed",
            Self::TargetWriteFailed => "target-write-failed",
            Self::ModuleLoadFailed => "module-load-failed",
            Self::InternalError => "internal-error",
        }
    }

    pub const fn sentence(self) -> &'static str {
        match self {
            Self::InstallationMediaNotFound => "No Yaoshi installer media was found.",
            Self::InstallationMediaAmbiguous => "More than one Yaoshi installer media was found.",
            Self::PayloadValidationFailed => "The Yaoshi payload did not validate.",
            Self::DiskInspectionFailed => "Disk inspection failed.",
            Self::SelectedDiskChanged => "The selected disk changed before writing started.",
            Self::TargetOpenFailed => "The selected disk cannot be opened for writing.",
            Self::TargetWriteFailed => "Installation copy failed.",
            Self::ModuleLoadFailed => "Installer device support failed to load.",
            Self::InternalError => "The installer hit an internal error.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareModel {
    pub display: NegotiatedDisplay,
    pub media_check_elapsed: Duration,
    pub modules_state: String,
    pub media_state: String,
    pub payload_state: String,
    pub disk_inspection_state: String,
    pub target_minimum_bytes: Option<u64>,
    pub payload_planned_extent_bytes: Option<u64>,
    pub payload_container_bytes: Option<u64>,
    pub payload_zero_extent_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetModel {
    pub candidates: Vec<TargetDiskCandidate>,
    pub selected_visible: usize,
    pub focus: TargetFocus,
    pub candidate_total_count: usize,
    pub selectable_count: usize,
    pub installer_media_count: usize,
    pub installed_target_count: usize,
    pub blocked_by_installed_target_count: usize,
    pub too_small_count: usize,
    pub unsupported_sector_size_count: usize,
    pub no_stable_id_count: usize,
    pub read_error_count: usize,
    pub target_minimum_bytes: u64,
    pub payload_planned_extent_bytes: u64,
    pub payload_container_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallModel {
    pub target: TargetDiskCandidate,
    pub target_minimum_bytes: u64,
    pub payload_planned_extent_bytes: u64,
    pub payload_container_bytes: u64,
    pub payload_zero_extent_bytes: u64,
    pub focus: InstallFocus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteModel {
    pub operation: TargetOperation,
    pub target_dev_path: PathBuf,
    pub task: WriteTask,
    pub head_scrub_written_bytes: u64,
    pub target_head_scrub_bytes: u64,
    pub tail_scrub_written_bytes: u64,
    pub target_tail_scrub_bytes: u64,
    pub planned_written_bytes: u64,
    pub planned_total_bytes: u64,
    pub source_read_bytes: u64,
    pub payload_source_bytes: u64,
    pub zero_written_bytes: u64,
    pub payload_zero_extent_bytes: u64,
    pub target_image_bytes: u64,
    pub current_rate_bps: Option<u64>,
    pub average_rate_bps: Option<u64>,
    pub eta: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneModel {
    pub operation: TargetOperation,
    pub target_dev_path: PathBuf,
    pub target_stable_id: Option<String>,
    pub payload_planned_extent_bytes: u64,
    pub payload_zero_extent_bytes: u64,
    pub focus: DoneFocus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoppedModel {
    pub reason: FailureReason,
    pub failed_step_name: String,
    pub affected_disk: Option<PathBuf>,
    pub failure_write_state: FailureWriteState,
    pub auto_poweroff_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitScene {
    Poweroff,
    Reboot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallerScene {
    Prepare(PrepareModel),
    Target(TargetModel),
    Install(InstallModel),
    Write(WriteModel),
    Done(DoneModel),
    Stopped(StoppedModel),
    Poweroff,
    Reboot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRegion {
    pub name: &'static str,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticSpan {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub semantic_style: SemanticStyle,
    pub fg: Color,
    pub bg: Color,
    pub modifiers: Modifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCell {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    pub semantic_style: SemanticStyle,
}

#[derive(Debug, Clone)]
pub struct RenderedFrame {
    pub cells_text: String,
    pub buffer: Buffer,
    pub semantic_spans: Vec<SemanticSpan>,
    pub region_tree: Vec<SemanticRegion>,
    pub focus_identity: Option<String>,
    pub normalized_cells: Vec<NormalizedCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardDiskRow {
    pub is_root: bool,
    pub dev: String,
    pub size: String,
    pub transport: String,
    pub vendor: String,
    pub serial: String,
    pub partitioned: String,
    pub fs: String,
    pub model: String,
    pub display: String,
}

impl DashboardDiskRow {
    fn alert_rank(&self) -> u8 {
        match metric_state(disk_fs_percent(&self.fs), 80.0, 90.0) {
            "critical" => 0,
            "warning" => 1,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardNetworkRow {
    pub is_route: bool,
    pub is_access: bool,
    pub iface: String,
    pub role: String,
    pub kind: String,
    pub state: String,
    pub addresses: String,
    pub mac: String,
    pub mtu: String,
    pub speed: String,
    pub rx_rate: String,
    pub tx_rate: String,
    pub rx_drop_rate: String,
    pub tx_drop_rate: String,
    pub alert_state: String,
}

impl DashboardNetworkRow {
    fn alert_rank(&self, network_error_state: &str) -> u8 {
        if network_error_state == "critical" {
            0
        } else if network_error_state == "warning" {
            1
        } else {
            2
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardCoreRow {
    pub index: usize,
    pub used_percent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardRouteRow {
    pub gateway: String,
    pub iface: String,
    pub address: String,
    pub metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSnapshot {
    pub display: NegotiatedDisplay,
    pub hostname: String,
    pub uptime: String,
    pub kernel_release: String,
    pub system_vendor: String,
    pub product_name: String,
    pub board_vendor: String,
    pub board_name: String,
    pub firmware_vendor: String,
    pub firmware_version: String,
    pub prepare_state: String,
    pub root_expansion_state: String,
    pub root_partuuid: String,
    pub root_label: String,
    pub ssh_ready_state: String,
    pub ssh_root_key_state: String,
    pub authorized_key_count: String,
    pub ssh_listening_state: String,
    pub ssh_process_state: String,
    pub networkd_process_state: String,
    pub dashboard_process_state: String,
    pub root_shell_state: String,
    pub rescue_shell_state: String,
    pub network_state: String,
    pub kernel_alert_state: String,
    pub kernel_warning_count: String,
    pub kernel_error_count: String,
    pub ssh_access_address: String,
    pub ssh_access_summary: String,
    pub route_iface: String,
    pub route_address: String,
    pub default_gateway: String,
    pub route_metric: String,
    pub cpu_used: String,
    pub cpu_model: String,
    pub logical_cpu_count: String,
    pub cpu_state: String,
    pub load1: String,
    pub load5: String,
    pub load15: String,
    pub memory_used_percent: String,
    pub memory_state: String,
    pub memory_used: String,
    pub memory_total: String,
    pub memory_dimm_summary: String,
    pub root_used_percent: String,
    pub root_state: String,
    pub root_used: String,
    pub root_total: String,
    pub root_inode_percent: String,
    pub inode_state: String,
    pub root_inode_usage: String,
    pub thermal: String,
    pub thermal_state: String,
    pub thermal_source: String,
    pub cpu_pressure: String,
    pub memory_pressure: String,
    pub io_pressure: String,
    pub top_cpu_process: String,
    pub process_count: String,
    pub running_process_count: String,
    pub blocked_process_count: String,
    pub memory_available: String,
    pub memory_cache: String,
    pub memory_dirty: String,
    pub swap_used: String,
    pub swap_total: String,
    pub swap_used_percent: String,
    pub top_rss_process: String,
    pub memory_full_pressure: String,
    pub io_full_pressure: String,
    pub disk_count: String,
    pub filesystem_rows: Vec<String>,
    pub network_interface_count: String,
    pub route_rows: Vec<DashboardRouteRow>,
    pub grow_root_service: String,
    pub ssh_service: String,
    pub networkd_service: String,
    pub dashboard_service: String,
    pub root_shell_service: String,
    pub disk_read_rate: String,
    pub disk_write_rate: String,
    pub disk_read_iops: String,
    pub disk_write_iops: String,
    pub network_rx_rate: String,
    pub network_tx_rate: String,
    pub network_rx_packet_rate: String,
    pub network_tx_packet_rate: String,
    pub network_rx_error_rate: String,
    pub network_tx_error_rate: String,
    pub network_rx_drop_rate: String,
    pub network_tx_drop_rate: String,
    pub network_error_state: String,
    pub interface_attention_count: String,
    pub core_rows: Vec<DashboardCoreRow>,
    pub disk_rows: Vec<DashboardDiskRow>,
    pub network_rows: Vec<DashboardNetworkRow>,
    pub disk_link_summary: String,
}

pub mod installer {
    use super::*;

    pub fn render(
        scene: InstallerScene,
        display: NegotiatedDisplay,
        size: (u16, u16),
    ) -> RenderedFrame {
        let _ = display;
        render_installer_scene(&scene, size.0, size.1)
    }

    pub fn scene(scene: &InstallerScene, size: (u16, u16)) -> RenderedFrame {
        render_installer_scene(scene, size.0, size.1)
    }
}

pub mod dashboard {
    use super::*;

    pub fn render(
        snapshot: DashboardSnapshot,
        display: NegotiatedDisplay,
        size: (u16, u16),
    ) -> RenderedFrame {
        let mut snapshot = snapshot;
        snapshot.display = display;
        render_dashboard_scene(&snapshot, size.0, size.1)
    }

    pub fn scene(snapshot: &DashboardSnapshot, size: (u16, u16)) -> RenderedFrame {
        render_dashboard_scene(snapshot, size.0, size.1)
    }
}
