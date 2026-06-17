#[derive(Debug, Clone)]
pub struct BuildSummary {
    pub image: &'static str,
    pub version: &'static str,
    pub size: u64,
    pub output: PathBuf,
    pub payload_bytes: u64,
    pub target_bytes: u64,
    pub planned_bytes: u64,
    pub zeroed_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct EffectiveProductConfig {
    pub hostname: String,
    pub debian_mirror: Url,
    pub debian_security_mirror: Url,
    pub ssh_authorized_keys: Vec<String>,
    pub installed_packages: Vec<String>,
    pub build_system_script_bytes: Vec<u8>,
    pub first_boot_script_bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct BuildContext {
    pub repo_root: PathBuf,
    pub state_root: PathBuf,
    pub work_root: PathBuf,
    pub cache_root: PathBuf,
    pub debian_download_cache: PathBuf,
    work_prepared: Cell<bool>,
}

impl BuildContext {
    fn new(repo_root: PathBuf) -> Self {
        let state_root = repo_root.join(STATE_ROOT);
        let work_root = repo_root.join(WORK_ROOT);
        let cache_root = state_root.join("cache");
        Self {
            repo_root,
            state_root: state_root.clone(),
            work_root,
            cache_root: cache_root.clone(),
            debian_download_cache: cache_root.join("apt-archives"),
            work_prepared: Cell::new(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPhase {
    BootstrapInstanceFiles,
    LoadConfig,
    ResolveCurrentImage,
    ResolveBuildIntents,
    ResolveCacheIndexGraph,
    PrepareWorkRootOnFirstProducerMiss,
    ResolveFoundationBranch,
    ResolveRootBranch,
    ResolveRuntimeBinaries,
    ResolveInstalledRuntimeBranch,
    ResolveInstallerEnvelopeBranch,
    ResolveFinalInstallerImage,
    PublishCurrentImage,
    PrintSummary,
}

pub const BUILD_PHASE_ORDER: &[BuildPhase] = &[
    BuildPhase::BootstrapInstanceFiles,
    BuildPhase::LoadConfig,
    BuildPhase::ResolveCurrentImage,
    BuildPhase::ResolveBuildIntents,
    BuildPhase::ResolveCacheIndexGraph,
    BuildPhase::PrepareWorkRootOnFirstProducerMiss,
    BuildPhase::ResolveFoundationBranch,
    BuildPhase::ResolveRootBranch,
    BuildPhase::ResolveRuntimeBinaries,
    BuildPhase::ResolveInstalledRuntimeBranch,
    BuildPhase::ResolveInstallerEnvelopeBranch,
    BuildPhase::ResolveFinalInstallerImage,
    BuildPhase::PublishCurrentImage,
    BuildPhase::PrintSummary,
];

impl BuildPhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::BootstrapInstanceFiles => "bootstrap-instance-files",
            Self::LoadConfig => "load-config",
            Self::ResolveCurrentImage => "resolve-current-image",
            Self::ResolveBuildIntents => "resolve-build-intents",
            Self::ResolveCacheIndexGraph => "resolve-cache-index-graph",
            Self::PrepareWorkRootOnFirstProducerMiss => "prepare-work-root-on-first-producer-miss",
            Self::ResolveFoundationBranch => "resolve-foundation-branch",
            Self::ResolveRootBranch => "resolve-root-branch",
            Self::ResolveRuntimeBinaries => "resolve-runtime-binaries",
            Self::ResolveInstalledRuntimeBranch => "resolve-installed-runtime-branch",
            Self::ResolveInstallerEnvelopeBranch => "resolve-installer-envelope-branch",
            Self::ResolveFinalInstallerImage => "resolve-final-installer-image",
            Self::PublishCurrentImage => "publish-current-image",
            Self::PrintSummary => "print-summary",
        }
    }
}

fn run_phase<T>(phase: BuildPhase, f: impl FnOnce() -> YaoshiResult<T>) -> YaoshiResult<T> {
    let _phase_name = phase.name();
    f()
}

#[derive(Debug)]
struct Paths {
    work: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
    root_helper: PathBuf,
    debian_package_root: PathBuf,
    customized_root: PathBuf,
    root_bridge_overlay: PathBuf,
    installed_root_source_file: PathBuf,
    installed_root_ext4: PathBuf,
    installer_modules: PathBuf,
    initramfs_base_tree: PathBuf,
    initramfs_app_tree: PathBuf,
    initramfs: PathBuf,
    installed_esp_tree: PathBuf,
    installer_boot_tree: PathBuf,
    image: PathBuf,
    debian_download_cache: PathBuf,
}

impl Paths {
    fn new(context: &BuildContext) -> Self {
        let work = context.work_root.clone();
        Self {
            runtime: work.join("bin"),
            root_helper: work.join("debian/.helper"),
            debian_package_root: work.join("debian/package-root"),
            customized_root: work.join("debian/customized-root"),
            root_bridge_overlay: work.join("debian/root-bridge-overlay"),
            installed_root_source_file: work.join("debian/installed-root-source/installed-root.tar"),
            installed_root_ext4: work.join("debian/installed-root-ext4"),
            installer_modules: work.join("debian/installer-modules"),
            initramfs_base_tree: work.join("initramfs/base.tree"),
            initramfs_app_tree: work.join("initramfs/app.tree"),
            initramfs: work.join("initramfs"),
            installed_esp_tree: work.join("image/installed-esp.tree"),
            installer_boot_tree: work.join("image/installer-boot.tree"),
            image: work.join("image"),
            debian_download_cache: context.debian_download_cache.clone(),
            cache: context.cache_root.clone(),
            work,
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeBinary {
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct RootBundle {
    installed_root_ext4: PathBuf,
    installed_root_ext4_tree_digest: String,
    boot: yaoshi_debian::DebianBootArtifacts,
    boot_export_digest: String,
    root_bytes: u64,
}

#[derive(Debug, Clone)]
struct PayloadArtifact {
    path: PathBuf,
    digest: String,
    info: yaoshi_payload::PayloadInfo,
}

pub fn run_from_current_dir() -> YaoshiResult<()> {
    let repo = std::env::current_dir()
        .map_err(|e| YaoshiError::internal(format!("resolve current directory: {e}")))?;
    let summary = run_build(&repo)?;
    run_phase(BuildPhase::PrintSummary, || {
        print_summary(&summary);
        Ok(())
    })
}

pub fn ensure_current_installer_image(repo: &Path) -> YaoshiResult<PathBuf> {
    ensure_current_installer_image_with_status(repo).map(|ready| ready.path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageReadyResult {
    CurrentHit,
    Rebuilt { reason: &'static str },
}

#[derive(Debug, Clone)]
pub struct ImageReady {
    pub path: PathBuf,
    pub result: ImageReadyResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentImageFingerprints {
    pub foundation: String,
    pub installed_root: String,
    pub installed_runtime: String,
    pub installer_envelope: String,
    pub published_image: String,
}

impl CurrentImageFingerprints {
    fn stamp_text(&self) -> String {
        format!(
            "foundation-fingerprint={}\ninstalled-root-fingerprint={}\ninstalled-runtime-fingerprint={}\ninstaller-envelope-fingerprint={}\npublished-image-fingerprint={}\n",
            self.foundation,
            self.installed_root,
            self.installed_runtime,
            self.installer_envelope,
            self.published_image
        )
    }
}

pub fn ensure_current_installer_image_with_status(repo: &Path) -> YaoshiResult<ImageReady> {
    let repo_root = absolute_repo_root(repo)?;
    bootstrap_instance_files(&repo_root)?;
    let config = load_config(&repo_root)?;
    let fingerprints = current_image_fingerprints(&repo_root, &config)?;
    let image = repo_root.join(OUTPUT_IMAGE);
    let stamp = repo_root.join(CURRENT_IMAGE_STAMP_PATH);
    match current_image_miss_reason(&image, &stamp, &fingerprints)? {
        None => {
            Ok(ImageReady {
                path: image,
                result: ImageReadyResult::CurrentHit,
            })
        }
        Some(reason) => {
            let summary = run_build(&repo_root)?;
            Ok(ImageReady {
                path: summary.output,
                result: ImageReadyResult::Rebuilt { reason },
            })
        }
    }
}

pub fn require_current_installer_image_with_status(repo: &Path) -> YaoshiResult<ImageReady> {
    let repo_root = absolute_repo_root(repo)?;
    bootstrap_instance_files(&repo_root)?;
    let config = load_config(&repo_root)?;
    let fingerprints = current_image_fingerprints(&repo_root, &config)?;
    let image = repo_root.join(OUTPUT_IMAGE);
    let stamp = repo_root.join(CURRENT_IMAGE_STAMP_PATH);
    match current_image_miss_reason(&image, &stamp, &fingerprints)? {
        None => Ok(ImageReady {
            path: image,
            result: ImageReadyResult::CurrentHit,
        }),
        Some(reason) => Err(YaoshiError::image(format!(
            "current installer image is not ready for qemu::flow: {reason}; run `cargo run --locked -p yaoshi --` first"
        ))),
    }
}

pub fn run_build(repo: &Path) -> YaoshiResult<BuildSummary> {
    let repo_root = absolute_repo_root(repo)?;
    run_phase(BuildPhase::BootstrapInstanceFiles, || {
        bootstrap_instance_files(&repo_root)
    })?;
    let config = run_phase(BuildPhase::LoadConfig, || load_config(&repo_root))?;
    let context = BuildContext::new(repo_root);
    reject_nonregular_output_path(&context)?;
    let fingerprints = current_image_fingerprints(&context.repo_root, &config)?;
    let current_image = context.repo_root.join(OUTPUT_IMAGE);
    let current_stamp = context.repo_root.join(CURRENT_IMAGE_STAMP_PATH);
    if let Some(summary) = run_phase(BuildPhase::ResolveCurrentImage, || {
        resolve_current_image_summary(&current_image, &current_stamp, &fingerprints)
    })? {
        return Ok(summary);
    }
    run_phase(BuildPhase::ResolveBuildIntents, resolve_build_intents)?;
    run_phase(BuildPhase::ResolveCacheIndexGraph, || {
        resolve_cache_index_graph(&context)
    })?;

    let debian_package_root = run_phase(BuildPhase::ResolveFoundationBranch, || {
        resolve_debian_package_root(&context, &config)
    })?;
    let root_bundle = run_phase(BuildPhase::ResolveRootBranch, || {
        resolve_root_branch(&context, &config, &debian_package_root)
    })?;
    let (installer_bin, dashboard_bin) = run_phase(BuildPhase::ResolveRuntimeBinaries, || {
        Ok((
            build_runtime_binary(&context, "yaoshi-installer")?,
            build_runtime_binary(&context, "yaoshi-dashboard")?,
        ))
    })?;
    let (_installed_esp, payload) = run_phase(BuildPhase::ResolveInstalledRuntimeBranch, || {
        stage_installed_esp_tree(&context, &config, &root_bundle.boot, &dashboard_bin.path)?;
        let installed_esp =
            pack_installed_esp_fat32(&context, &config, &root_bundle, &dashboard_bin.path)?;
        let payload =
            pack_installed_system_payload_from_target_graph(&context, &installed_esp, &root_bundle)?;
        Ok((installed_esp, payload))
    })?;
    let installer_boot = run_phase(BuildPhase::ResolveInstallerEnvelopeBranch, || {
        let module_closure = compute_installer_module_closure(&context, &root_bundle.boot)?;
        let base_entries = render_installer_base_initramfs_tree(&context, &module_closure)?;
        let app_entries = render_installer_app_initramfs_tree(&context, &installer_bin.path)?;
        let base_newc = pack_installer_base_initramfs_newc(&context, &base_entries)?;
        let app_newc = pack_installer_app_initramfs_newc(&context, &app_entries)?;
        let base_initramfs_zstd = compress_installer_base_initramfs_zstd(&context, &base_newc)?;
        let app_initramfs_zstd = compress_installer_app_initramfs_zstd(&context, &app_newc)?;
        stage_installer_boot_tree(
            &context,
            &root_bundle.boot,
            &base_initramfs_zstd,
            &app_initramfs_zstd,
        )?;
        pack_installer_boot_fat32(
            &context,
            &root_bundle,
            &base_initramfs_zstd,
            &app_initramfs_zstd,
        )
    })?;
    let final_installer = run_phase(BuildPhase::ResolveFinalInstallerImage, || {
        pack_final_installer_mbr_composite(&context, &installer_boot, &payload)
    })?;
    let output = run_phase(BuildPhase::PublishCurrentImage, || {
        publish(&context, &config, &final_installer)
    })?;
    let size = fs::metadata(&output)
        .map_err(|e| YaoshiError::publish(format!("stat published image: {e}")))?
        .len();
    Ok(summary_from_payload(output, size, &payload.info))
}

fn resolve_current_image_summary(
    current_image: &Path,
    current_stamp: &Path,
    fingerprints: &CurrentImageFingerprints,
) -> YaoshiResult<Option<BuildSummary>> {
    if current_image.is_file()
        && stamp_matches(current_stamp, fingerprints)?
        && let Ok(payload) = qemu_preflight_current_image(current_image)
    {
        let size = fs::metadata(current_image)
            .map_err(|e| YaoshiError::publish(format!("stat current image: {e}")))?
            .len();
        return Ok(Some(summary_from_payload(
            current_image.to_path_buf(),
            size,
            &payload,
        )));
    }
    Ok(None)
}

fn current_image_miss_reason(
    current_image: &Path,
    current_stamp: &Path,
    fingerprints: &CurrentImageFingerprints,
) -> YaoshiResult<Option<&'static str>> {
    let Ok(text) = fs::read_to_string(current_stamp) else {
        return Ok(Some("missing-stamp"));
    };
    let Some(parsed) = parse_current_image_stamp(&text) else {
        return Ok(Some("malformed-stamp"));
    };
    if &parsed != fingerprints {
        return Ok(Some("stale-stamp"));
    }
    if !current_image.is_file() {
        return Ok(Some("missing-image"));
    }
    if qemu_preflight_current_image(current_image).is_err() {
        return Ok(Some("media-check-failed"));
    }
    Ok(None)
}

fn summary_from_payload(
    output: PathBuf,
    size: u64,
    payload: &yaoshi_payload::PayloadInfo,
) -> BuildSummary {
    BuildSummary {
        image: PRODUCT_NAME,
        version: VERSION,
        size,
        output,
        payload_bytes: payload.total_payload_bytes,
        target_bytes: payload.target_image_bytes,
        planned_bytes: payload.planned_extent_bytes,
        zeroed_bytes: payload.planned_zero_bytes,
    }
}

fn print_summary(summary: &BuildSummary) {
    println!("Image: {}", summary.image);
    println!("Version: {}", summary.version);
    println!("Size: {}", summary.size);
    println!("Output: {}", OUTPUT_IMAGE);
    println!("Payload: {}", summary.payload_bytes);
    println!("Target: {}", summary.target_bytes);
    println!("Planned: {}", summary.planned_bytes);
    println!("Zeroed: {}", summary.zeroed_bytes);
}

pub fn bootstrap_instance_files(repo: &Path) -> YaoshiResult<()> {
    let state = repo.join(STATE_ROOT);
    ensure_bootstrap_dir(&state)?;
    let scripts = state.join("scripts");
    ensure_bootstrap_dir(&scripts)?;
    bootstrap_rendered_file(
        &repo.join(CONFIG_PATH),
        templates::render_config_yaoshi_toml()
            .map_err(|e| YaoshiError::config(format!("render config/yaoshi.toml.askama: {e}")))?,
    )?;
    bootstrap_rendered_file(
        &repo.join(".yaoshi/scripts/build-system.sh"),
        templates::render_config_build_system_sh().map_err(|e| {
            YaoshiError::config(format!("render config/build-system.sh.askama: {e}"))
        })?,
    )?;
    bootstrap_rendered_file(
        &repo.join(".yaoshi/scripts/first-boot.sh"),
        templates::render_config_first_boot_sh()
            .map_err(|e| YaoshiError::config(format!("render config/first-boot.sh.askama: {e}")))?,
    )
}
