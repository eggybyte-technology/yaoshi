use yaoshi_common::{CandidateStatus, TargetDiskCandidate};
use yaoshi_screen::{
    DoneFocus, FailureWriteState, Input, InstallFocus, TargetFocus, TargetOperation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerPhase {
    Start,
    DiscoverSource,
    InspectDisks,
    SelectDisk,
    ConfirmDisk,
    WriteTarget,
    Complete,
    Failure(FailureReason),
    ControlledExit(ExitAction),
    Halted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitAction {
    Poweroff,
    Reboot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    SourceNotFound,
    SourceAmbiguous,
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
            Self::SourceNotFound => "installation-media-not-found",
            Self::SourceAmbiguous => "installation-media-ambiguous",
            Self::DiskInspectionFailed => "disk-inspection-failed",
            Self::SelectedDiskChanged => "selected-disk-changed",
            Self::TargetOpenFailed => "target-open-failed",
            Self::TargetWriteFailed => "target-write-failed",
            Self::ModuleLoadFailed => "module-load-failed",
            Self::InternalError => "internal-error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectRequest {
    DiscoverMedia,
    InspectDisks,
    StartWrite,
    StartErase,
    Power(ExitAction),
    None,
}

#[derive(Debug, Clone)]
pub struct InstallerMachine {
    pub phase: InstallerPhase,
    pub target_focus: TargetFocus,
    pub install_focus: InstallFocus,
    pub done_focus: DoneFocus,
    pub completed_operation: TargetOperation,
    pub selected_visible: usize,
    pub failure_write_state: FailureWriteState,
    pub candidates: Vec<TargetDiskCandidate>,
}

impl Default for InstallerMachine {
    fn default() -> Self {
        Self {
            phase: InstallerPhase::Start,
            target_focus: TargetFocus::Refresh,
            install_focus: InstallFocus::Back,
            done_focus: DoneFocus::Reboot,
            completed_operation: TargetOperation::Install,
            selected_visible: 0,
            failure_write_state: FailureWriteState::NoTargetWrites,
            candidates: Vec::new(),
        }
    }
}

impl InstallerMachine {
    pub fn first_frame_flushed(&mut self) -> EffectRequest {
        self.phase = InstallerPhase::DiscoverSource;
        EffectRequest::DiscoverMedia
    }

    pub fn media_discovered(&mut self) -> EffectRequest {
        self.phase = InstallerPhase::InspectDisks;
        EffectRequest::InspectDisks
    }

    pub fn disk_inventory_ready(&mut self, candidates: Vec<TargetDiskCandidate>) -> EffectRequest {
        self.phase = InstallerPhase::SelectDisk;
        self.candidates = candidates;
        self.selected_visible = 0;
        self.target_focus = if self.selectable_count() == 0 {
            TargetFocus::Refresh
        } else {
            TargetFocus::ConfirmTarget
        };
        EffectRequest::None
    }

    pub fn input(&mut self, input: Input) -> EffectRequest {
        match self.phase {
            InstallerPhase::SelectDisk => self.target_input(input),
            InstallerPhase::ConfirmDisk => self.install_input(input),
            InstallerPhase::Complete => self.done_input(input),
            InstallerPhase::DiscoverSource if input == Input::Enter => {
                self.phase = InstallerPhase::ControlledExit(ExitAction::Poweroff);
                EffectRequest::Power(ExitAction::Poweroff)
            }
            InstallerPhase::Failure(_) if input == Input::Enter => {
                self.phase = InstallerPhase::ControlledExit(ExitAction::Poweroff);
                EffectRequest::Power(ExitAction::Poweroff)
            }
            _ => EffectRequest::None,
        }
    }

    pub fn enter_failure(&mut self, writes_started: bool) {
        self.phase = InstallerPhase::Failure(FailureReason::InternalError);
        self.failure_write_state = if writes_started {
            FailureWriteState::TargetWriteStarted
        } else {
            FailureWriteState::NoTargetWrites
        };
    }

    pub fn write_started(&mut self) {
        self.phase = InstallerPhase::WriteTarget;
        self.failure_write_state = FailureWriteState::TargetWriteStarted;
    }

    pub fn write_completed(&mut self) {
        self.phase = InstallerPhase::Complete;
        self.completed_operation = TargetOperation::Install;
        self.done_focus = DoneFocus::Reboot;
    }

    pub fn erase_completed(&mut self) {
        self.phase = InstallerPhase::Complete;
        self.completed_operation = TargetOperation::Erase;
        self.done_focus = DoneFocus::ChooseTarget;
    }

    fn target_input(&mut self, input: Input) -> EffectRequest {
        let selectable = self.selectable_count();
        match input {
            Input::ArrowUp if selectable > 0 => {
                self.selected_visible = self.selected_visible.saturating_sub(1);
            }
            Input::ArrowDown if selectable > 0 => {
                if self.selected_visible + 1 < selectable {
                    self.selected_visible += 1;
                }
            }
            Input::Tab => self.target_focus = next_target_focus(self.target_focus, selectable > 0),
            Input::Enter => match self.target_focus {
                TargetFocus::ConfirmTarget if selectable > 0 => {
                    self.phase = InstallerPhase::ConfirmDisk;
                    self.install_focus = InstallFocus::Back;
                }
                TargetFocus::Refresh => {
                    self.phase = InstallerPhase::InspectDisks;
                    return EffectRequest::InspectDisks;
                }
                TargetFocus::PowerOff => {
                    self.phase = InstallerPhase::ControlledExit(ExitAction::Poweroff);
                    return EffectRequest::Power(ExitAction::Poweroff);
                }
                _ => {}
            },
            _ => {}
        }
        EffectRequest::None
    }

    fn install_input(&mut self, input: Input) -> EffectRequest {
        match input {
            Input::Tab => {
                self.install_focus = match self.install_focus {
                    InstallFocus::Back => InstallFocus::Erase,
                    InstallFocus::Erase => InstallFocus::Destructive,
                    InstallFocus::Destructive => InstallFocus::PowerOff,
                    InstallFocus::PowerOff => InstallFocus::Back,
                };
                EffectRequest::None
            }
            Input::Enter if self.install_focus == InstallFocus::Back => {
                self.phase = InstallerPhase::SelectDisk;
                EffectRequest::None
            }
            Input::Enter if self.install_focus == InstallFocus::Erase => EffectRequest::StartErase,
            Input::Enter if self.install_focus == InstallFocus::Destructive => {
                EffectRequest::StartWrite
            }
            Input::Enter if self.install_focus == InstallFocus::PowerOff => {
                self.phase = InstallerPhase::ControlledExit(ExitAction::Poweroff);
                EffectRequest::Power(ExitAction::Poweroff)
            }
            Input::Enter => EffectRequest::None,
            _ => EffectRequest::None,
        }
    }

    fn done_input(&mut self, input: Input) -> EffectRequest {
        match input {
            Input::Tab => {
                self.done_focus = next_done_focus(self.completed_operation, self.done_focus);
                EffectRequest::None
            }
            Input::Enter
                if self.done_focus == DoneFocus::ChooseTarget
                    && self.completed_operation == TargetOperation::Erase =>
            {
                self.phase = InstallerPhase::InspectDisks;
                EffectRequest::InspectDisks
            }
            Input::Enter => {
                let action = match self.done_focus {
                    DoneFocus::ChooseTarget => ExitAction::Reboot,
                    DoneFocus::Reboot => ExitAction::Reboot,
                    DoneFocus::PowerOff => ExitAction::Poweroff,
                };
                self.phase = InstallerPhase::ControlledExit(action);
                EffectRequest::Power(action)
            }
            _ => EffectRequest::None,
        }
    }

    fn selectable_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.status == CandidateStatus::Selectable)
            .count()
    }
}

fn next_done_focus(operation: TargetOperation, focus: DoneFocus) -> DoneFocus {
    match operation {
        TargetOperation::Install => match focus {
            DoneFocus::PowerOff => DoneFocus::Reboot,
            _ => DoneFocus::PowerOff,
        },
        TargetOperation::Erase => match focus {
            DoneFocus::ChooseTarget => DoneFocus::Reboot,
            DoneFocus::Reboot => DoneFocus::PowerOff,
            DoneFocus::PowerOff => DoneFocus::ChooseTarget,
        },
    }
}

fn next_target_focus(focus: TargetFocus, has_disks: bool) -> TargetFocus {
    if has_disks {
        match focus {
            TargetFocus::ConfirmTarget => TargetFocus::Refresh,
            TargetFocus::Refresh => TargetFocus::PowerOff,
            TargetFocus::PowerOff => TargetFocus::ConfirmTarget,
        }
    } else {
        match focus {
            TargetFocus::Refresh => TargetFocus::PowerOff,
            _ => TargetFocus::Refresh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_disk(name: &str, bytes: u64) -> TargetDiskCandidate {
        TargetDiskCandidate {
            disk: yaoshi_common::KernelDiskRef {
                sysfs_path: PathBuf::from(format!("/sys/block/{name}")),
                major_minor: "8:0".to_string(),
                kernel_name: name.to_string(),
                dev_path: PathBuf::from(format!("/dev/{name}")),
                logical_block_size: 512,
                byte_size: bytes,
                model: Some("Yaoshi Test Disk".to_string()),
                serial: Some(format!("SERIAL-{name}")),
                stable_disk_id: Some(PathBuf::from(format!(
                    "/dev/disk/by-id/virtio-{name}-stable"
                ))),
            },
            status: CandidateStatus::Selectable,
            existing: yaoshi_common::ExistingPartitionTable::Gpt,
        }
    }

    #[test]
    fn install_enters_with_back_focused() {
        let mut machine = InstallerMachine::default();
        machine.disk_inventory_ready(vec![fixture_disk("vda", 4 << 30)]);
        machine.input(Input::Enter);
        assert_eq!(machine.phase, InstallerPhase::ConfirmDisk);
        assert_eq!(machine.install_focus, InstallFocus::Back);
    }

    #[test]
    fn target_arrows_do_not_wrap() {
        let mut machine = InstallerMachine::default();
        machine.disk_inventory_ready(vec![
            fixture_disk("vda", 4 << 30),
            fixture_disk("vdb", 4 << 30),
        ]);
        machine.input(Input::ArrowUp);
        assert_eq!(machine.selected_visible, 0);
        machine.input(Input::ArrowDown);
        machine.input(Input::ArrowDown);
        assert_eq!(machine.selected_visible, 1);
    }

    #[test]
    fn write_input_is_locked() {
        let mut machine = InstallerMachine::default();
        machine.write_started();
        assert_eq!(machine.input(Input::Enter), EffectRequest::None);
        assert_eq!(machine.phase, InstallerPhase::WriteTarget);
    }

    #[test]
    fn confirm_install_exposes_erase_before_install() {
        let mut machine = InstallerMachine::default();
        machine.disk_inventory_ready(vec![fixture_disk("vda", 4 << 30)]);
        machine.input(Input::Enter);
        assert_eq!(machine.phase, InstallerPhase::ConfirmDisk);
        assert_eq!(machine.install_focus, InstallFocus::Back);

        machine.input(Input::Tab);
        assert_eq!(machine.install_focus, InstallFocus::Erase);
        assert_eq!(machine.input(Input::Enter), EffectRequest::StartErase);

        machine.input(Input::Tab);
        assert_eq!(machine.install_focus, InstallFocus::Destructive);
        assert_eq!(machine.input(Input::Enter), EffectRequest::StartWrite);
    }

    #[test]
    fn erase_completion_returns_to_target_selection() {
        let mut machine = InstallerMachine::default();
        machine.erase_completed();
        assert_eq!(machine.phase, InstallerPhase::Complete);
        assert_eq!(machine.done_focus, DoneFocus::ChooseTarget);
        assert_eq!(machine.input(Input::Enter), EffectRequest::InspectDisks);
        assert_eq!(machine.phase, InstallerPhase::InspectDisks);
    }

    #[test]
    fn install_completion_still_exits() {
        let mut machine = InstallerMachine::default();
        machine.write_completed();
        assert_eq!(machine.done_focus, DoneFocus::Reboot);
        assert_eq!(
            machine.input(Input::Enter),
            EffectRequest::Power(ExitAction::Reboot)
        );
    }
}
