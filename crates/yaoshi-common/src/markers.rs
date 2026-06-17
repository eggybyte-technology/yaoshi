pub const FIRST_FRAME_FLUSHED: &str = "YAOSHI_MARK v=1 kind=installer event=first_frame_flushed";
pub const MODULE_LOADING_STARTED: &str =
    "YAOSHI_MARK v=1 kind=installer event=module_loading_started";
pub const MODULE_LOADING_COMPLETE: &str =
    "YAOSHI_MARK v=1 kind=installer event=module_loading_complete";
pub const INSTALLATION_MEDIA_DISCOVERED: &str =
    "YAOSHI_MARK v=1 kind=installer event=installation_media_discovered";
pub const TARGET_WRITE_STARTED: &str = "YAOSHI_MARK v=1 kind=installer event=target_write_started";
pub const COMPLETE: &str = "YAOSHI_MARK v=1 kind=installer event=complete";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTask {
    VerifyTarget,
    PrepareDiskBeginning,
    PrepareDiskEnd,
    CopyYaoshiImage,
    FinalizeWrites,
}

impl WriteTask {
    pub const fn token(self) -> &'static str {
        match self {
            Self::VerifyTarget => "verify-target",
            Self::PrepareDiskBeginning => "prepare-disk-beginning",
            Self::PrepareDiskEnd => "prepare-disk-end",
            Self::CopyYaoshiImage => "copy-yaoshi-image",
            Self::FinalizeWrites => "finalize-writes",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifyTarget => "verify target",
            Self::PrepareDiskBeginning => "prepare disk beginning",
            Self::PrepareDiskEnd => "prepare disk end",
            Self::CopyYaoshiImage => "copy Yaoshi image",
            Self::FinalizeWrites => "finalize writes",
        }
    }

    pub const fn ordered() -> [Self; 5] {
        [
            Self::VerifyTarget,
            Self::PrepareDiskBeginning,
            Self::PrepareDiskEnd,
            Self::CopyYaoshiImage,
            Self::FinalizeWrites,
        ]
    }
}

pub fn controlled_exit(action: &str) -> String {
    format!("YAOSHI_MARK v=1 kind=installer event=controlled_exit action={action}")
}

pub fn module_loading_module_started(module: &str) -> String {
    format!("YAOSHI_MARK v=1 kind=installer event=module_loading_module_started module={module}")
}

pub fn module_loading_module_complete(module: &str) -> String {
    format!("YAOSHI_MARK v=1 kind=installer event=module_loading_module_complete module={module}")
}

pub fn module_loading_module_failed(module: &str, reason: &str) -> String {
    format!(
        "YAOSHI_MARK v=1 kind=installer event=module_loading_module_failed module={module} reason={reason}"
    )
}

pub fn write_task(task: WriteTask, state: &str) -> String {
    format!(
        "YAOSHI_MARK v=1 kind=installer event=write_task task={} state={state}",
        task.token()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteProgress {
    pub processed_extent_count: u64,
    pub payload_extent_count: u64,
    pub planned_written_bytes: u64,
    pub payload_planned_extent_bytes: u64,
    pub source_read_bytes: u64,
    pub payload_source_bytes: u64,
    pub zero_written_bytes: u64,
    pub payload_zero_extent_bytes: u64,
}

pub fn write_progress(progress: WriteProgress) -> String {
    let percent = if progress.payload_planned_extent_bytes == 0 {
        0.0
    } else {
        (progress.planned_written_bytes as f64 * 100.0
            / progress.payload_planned_extent_bytes as f64)
            .clamp(0.0, 100.0)
    };
    let WriteProgress {
        processed_extent_count,
        payload_extent_count,
        planned_written_bytes,
        payload_planned_extent_bytes,
        source_read_bytes,
        payload_source_bytes,
        zero_written_bytes,
        payload_zero_extent_bytes,
    } = progress;
    format!(
        "YAOSHI_MARK v=1 kind=installer event=write_progress task=copy-yaoshi-image extent={processed_extent_count}/{payload_extent_count} written={planned_written_bytes}/{payload_planned_extent_bytes} source={source_read_bytes}/{payload_source_bytes} zeroed={zero_written_bytes}/{payload_zero_extent_bytes} percent={percent:.3}%"
    )
}

pub fn screen_frame_begin(screen_id: &str, columns: u16, rows: u16) -> String {
    format!("YAOSHI_SCREEN_FRAME_BEGIN v=1 screen={screen_id} columns={columns} rows={rows}")
}

pub fn screen_frame_row(screen_id: &str, row: usize, text: &str) -> String {
    format!("YAOSHI_SCREEN_FRAME_ROW v=1 screen={screen_id} row={row} text={text}")
}

pub fn screen_frame_style(screen_id: &str, row: usize, text: &str) -> String {
    format!("YAOSHI_SCREEN_FRAME_STYLE v=1 screen={screen_id} row={row} text={text}")
}

pub fn screen_frame_end(screen_id: &str) -> String {
    format!("YAOSHI_SCREEN_FRAME_END v=1 screen={screen_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_progress_includes_zero_current_and_total() {
        let line = write_progress(WriteProgress {
            processed_extent_count: 3,
            payload_extent_count: 10,
            planned_written_bytes: 4096,
            payload_planned_extent_bytes: 8192,
            source_read_bytes: 1024,
            payload_source_bytes: 2048,
            zero_written_bytes: 512,
            payload_zero_extent_bytes: 1536,
        });
        assert!(line.contains("task=copy-yaoshi-image"));
        assert!(line.contains("zeroed=512/1536"));
        assert!(line.contains("percent=50.000%"));
    }

    #[test]
    fn write_task_markers_use_five_task_transaction() {
        let tokens = WriteTask::ordered()
            .into_iter()
            .map(WriteTask::token)
            .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                "verify-target",
                "prepare-disk-beginning",
                "prepare-disk-end",
                "copy-yaoshi-image",
                "finalize-writes",
            ]
        );
        assert_eq!(
            write_task(WriteTask::VerifyTarget, "active"),
            "YAOSHI_MARK v=1 kind=installer event=write_task task=verify-target state=active"
        );
    }

    #[test]
    fn module_loading_markers_include_module_path() {
        let module = "lib/modules/6.12.0/kernel/drivers/usb/storage/usb-storage.ko";
        assert_eq!(
            module_loading_module_started(module),
            "YAOSHI_MARK v=1 kind=installer event=module_loading_module_started module=lib/modules/6.12.0/kernel/drivers/usb/storage/usb-storage.ko"
        );
        assert_eq!(
            module_loading_module_complete(module),
            "YAOSHI_MARK v=1 kind=installer event=module_loading_module_complete module=lib/modules/6.12.0/kernel/drivers/usb/storage/usb-storage.ko"
        );
        assert_eq!(
            module_loading_module_failed(module, "timed_out_after_30s"),
            "YAOSHI_MARK v=1 kind=installer event=module_loading_module_failed module=lib/modules/6.12.0/kernel/drivers/usb/storage/usb-storage.ko reason=timed_out_after_30s"
        );
    }
}
