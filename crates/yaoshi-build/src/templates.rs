use askama::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateGroup {
    ConfigBootstrap,
    DebianHelper,
    RootOverlay,
    InstalledEsp,
    InstallerBoot,
}

pub(crate) const CONFIG_BOOTSTRAP_CONTEXT_GRAMMAR: &str = "yaoshi.askama.config-bootstrap.v1";
pub(crate) const DEBIAN_HELPER_CONTEXT_GRAMMAR: &str = "yaoshi.askama.debian-helper.v1";
pub(crate) const ROOT_OVERLAY_CONTEXT_GRAMMAR: &str = "yaoshi.askama.root-overlay.v1";
pub(crate) const INSTALLED_ESP_CONTEXT_GRAMMAR: &str = "yaoshi.askama.installed-esp.v1";
pub(crate) const INSTALLER_BOOT_CONTEXT_GRAMMAR: &str = "yaoshi.askama.installer-boot.v1";

#[derive(Template)]
#[template(path = "config/yaoshi.toml.askama", escape = "none")]
struct ConfigYaoshiToml;

#[derive(Template)]
#[template(path = "config/build-system.sh.askama", escape = "none")]
struct ConfigBuildSystemSh;

#[derive(Template)]
#[template(path = "config/first-boot.sh.askama", escape = "none")]
struct ConfigFirstBootSh;

#[derive(Template)]
#[template(path = "debian/build-system-runner.sh.askama", escape = "none")]
struct DebianBuildSystemRunnerSh;

#[derive(Template)]
#[template(path = "debian/root-export-helper.sh.askama", escape = "none")]
struct DebianRootExportHelperSh;

#[derive(Template)]
#[template(path = "debian/mke2fs.conf.askama", escape = "none")]
struct DebianMke2fsConf;

#[derive(Template)]
#[template(path = "overlay/prepare-launcher.sh.askama", escape = "none")]
struct OverlayPrepareLauncherSh;

#[derive(Template)]
#[template(path = "overlay/first-boot-launcher.sh.askama", escape = "none")]
struct OverlayFirstBootLauncherSh;

#[derive(Template)]
#[template(path = "overlay/fstab.askama", escape = "none")]
struct OverlayFstab;

#[derive(Template)]
#[template(path = "overlay/hostname.askama", escape = "none")]
struct OverlayHostname;

#[derive(Template)]
#[template(
    path = "overlay/systemd/yaoshi-dashboard.service.askama",
    escape = "none"
)]
struct OverlayYaoshiDashboardService;

#[derive(Template)]
#[template(
    path = "overlay/systemd/yaoshi-root-shell.service.askama",
    escape = "none"
)]
struct OverlayYaoshiRootShellService;

#[derive(Template)]
#[template(
    path = "overlay/systemd/yaoshi-prepare.service.askama",
    escape = "none"
)]
struct OverlayYaoshiPrepareService;

#[derive(Template)]
#[template(
    path = "overlay/systemd/yaoshi-first-boot.service.askama",
    escape = "none"
)]
struct OverlayYaoshiFirstBootService;

#[derive(Template)]
#[template(path = "overlay/systemd/ssh-order.conf.askama", escape = "none")]
struct OverlaySshOrderConf;

#[derive(Template)]
#[template(
    path = "overlay/network/20-yaoshi-dhcp.network.askama",
    escape = "none"
)]
struct OverlayNetworkDhcp;

#[derive(Template)]
#[template(path = "overlay/ssh/10-yaoshi.conf.askama", escape = "none")]
struct OverlaySshdConfig;

#[derive(Template)]
#[template(path = "overlay/repart/20-yaoshi-root.conf.askama", escape = "none")]
struct OverlayRepartRoot;

#[derive(Template)]
#[template(path = "esp/boot/KERNEL-RELEASE.askama", escape = "none")]
struct EspKernelRelease<'a> {
    kernel_release: &'a str,
}

#[derive(Template)]
#[template(path = "esp/config/HOSTNAME.askama", escape = "none")]
struct EspHostname<'a> {
    hostname: &'a str,
}

#[derive(Template)]
#[template(path = "esp/config/AUTHKEYS.askama", escape = "none")]
struct EspAuthorizedKeys<'a> {
    authorized_keys: &'a str,
}

#[derive(Template)]
#[template(path = "esp/runtime/PREPARE.askama", escape = "none")]
struct EspPrepare;

#[derive(Template)]
#[template(path = "esp/loader/loader.conf.askama", escape = "none")]
struct EspLoaderConf;

#[derive(Template)]
#[template(path = "esp/loader/yaoshi.conf.askama", escape = "none")]
struct EspYaoshiConf;

#[derive(Template)]
#[template(path = "installer-boot/boot/KERNEL-RELEASE.askama", escape = "none")]
struct InstallerBootKernelRelease<'a> {
    kernel_release: &'a str,
}

#[derive(Template)]
#[template(path = "installer-boot/loader/loader.conf.askama", escape = "none")]
struct InstallerBootLoaderConf;

#[derive(Template)]
#[template(
    path = "installer-boot/loader/yaoshi-installer.conf.askama",
    escape = "none"
)]
struct InstallerBootYaoshiInstallerConf;

fn render_template(template: impl Template) -> Result<String, askama::Error> {
    template.render()
}

pub(crate) fn render_config_yaoshi_toml() -> Result<String, askama::Error> {
    render_template(ConfigYaoshiToml)
}

pub(crate) fn render_config_build_system_sh() -> Result<String, askama::Error> {
    render_template(ConfigBuildSystemSh)
}

pub(crate) fn render_config_first_boot_sh() -> Result<String, askama::Error> {
    render_template(ConfigFirstBootSh)
}

pub(crate) fn render_debian_build_system_runner_sh() -> Result<String, askama::Error> {
    render_template(DebianBuildSystemRunnerSh)
}

pub(crate) fn render_debian_root_export_helper_sh() -> Result<String, askama::Error> {
    render_template(DebianRootExportHelperSh)
}

pub(crate) fn render_debian_mke2fs_conf() -> Result<String, askama::Error> {
    render_template(DebianMke2fsConf)
}

pub(crate) fn render_overlay_prepare_launcher_sh() -> Result<String, askama::Error> {
    render_template(OverlayPrepareLauncherSh)
}

pub(crate) fn render_overlay_first_boot_launcher_sh() -> Result<String, askama::Error> {
    render_template(OverlayFirstBootLauncherSh)
}

pub(crate) fn render_overlay_fstab() -> Result<String, askama::Error> {
    render_template(OverlayFstab)
}

pub(crate) fn render_overlay_hostname() -> Result<String, askama::Error> {
    render_template(OverlayHostname)
}

pub(crate) fn render_overlay_yaoshi_dashboard_service() -> Result<String, askama::Error> {
    render_template(OverlayYaoshiDashboardService)
}

pub(crate) fn render_overlay_yaoshi_root_shell_service() -> Result<String, askama::Error> {
    render_template(OverlayYaoshiRootShellService)
}

pub(crate) fn render_overlay_yaoshi_prepare_service() -> Result<String, askama::Error> {
    render_template(OverlayYaoshiPrepareService)
}

pub(crate) fn render_overlay_yaoshi_first_boot_service() -> Result<String, askama::Error> {
    render_template(OverlayYaoshiFirstBootService)
}

pub(crate) fn render_overlay_ssh_order_conf() -> Result<String, askama::Error> {
    render_template(OverlaySshOrderConf)
}

pub(crate) fn render_overlay_network_dhcp() -> Result<String, askama::Error> {
    render_template(OverlayNetworkDhcp)
}

pub(crate) fn render_overlay_sshd_config() -> Result<String, askama::Error> {
    render_template(OverlaySshdConfig)
}

pub(crate) fn render_overlay_repart_root() -> Result<String, askama::Error> {
    render_template(OverlayRepartRoot)
}

pub(crate) fn render_esp_kernel_release(kernel_release: &str) -> Result<String, askama::Error> {
    render_template(EspKernelRelease { kernel_release })
}

pub(crate) fn render_esp_hostname(hostname: &str) -> Result<String, askama::Error> {
    render_template(EspHostname { hostname })
}

pub(crate) fn render_esp_authorized_keys(keys: &[String]) -> Result<String, askama::Error> {
    render_template(EspAuthorizedKeys {
        authorized_keys: &keys.join("\n"),
    })
}

pub(crate) fn render_esp_prepare() -> Result<String, askama::Error> {
    render_template(EspPrepare)
}

pub(crate) fn render_esp_loader_conf() -> Result<String, askama::Error> {
    render_template(EspLoaderConf)
}

pub(crate) fn render_esp_yaoshi_conf() -> Result<String, askama::Error> {
    render_template(EspYaoshiConf)
}

pub(crate) fn render_installer_boot_kernel_release(
    kernel_release: &str,
) -> Result<String, askama::Error> {
    render_template(InstallerBootKernelRelease { kernel_release })
}

pub(crate) fn render_installer_boot_loader_conf() -> Result<String, askama::Error> {
    render_template(InstallerBootLoaderConf)
}

pub(crate) fn render_installer_boot_yaoshi_installer_conf() -> Result<String, askama::Error> {
    render_template(InstallerBootYaoshiInstallerConf)
}

pub(crate) fn template_source_files(
    group: TemplateGroup,
) -> &'static [(&'static str, &'static [u8])] {
    match group {
        TemplateGroup::ConfigBootstrap => &CONFIG_BOOTSTRAP_SOURCES,
        TemplateGroup::DebianHelper => &DEBIAN_HELPER_SOURCES,
        TemplateGroup::RootOverlay => &ROOT_OVERLAY_SOURCES,
        TemplateGroup::InstalledEsp => &INSTALLED_ESP_SOURCES,
        TemplateGroup::InstallerBoot => &INSTALLER_BOOT_SOURCES,
    }
}

const CONFIG_BOOTSTRAP_SOURCES: [(&str, &[u8]); 3] = [
    (
        "config/yaoshi.toml.askama",
        include_bytes!("../templates/config/yaoshi.toml.askama"),
    ),
    (
        "config/build-system.sh.askama",
        include_bytes!("../templates/config/build-system.sh.askama"),
    ),
    (
        "config/first-boot.sh.askama",
        include_bytes!("../templates/config/first-boot.sh.askama"),
    ),
];

const DEBIAN_HELPER_SOURCES: [(&str, &[u8]); 3] = [
    (
        "debian/build-system-runner.sh.askama",
        include_bytes!("../templates/debian/build-system-runner.sh.askama"),
    ),
    (
        "debian/root-export-helper.sh.askama",
        include_bytes!("../templates/debian/root-export-helper.sh.askama"),
    ),
    (
        "debian/mke2fs.conf.askama",
        include_bytes!("../templates/debian/mke2fs.conf.askama"),
    ),
];

const ROOT_OVERLAY_SOURCES: [(&str, &[u8]); 12] = [
    (
        "overlay/prepare-launcher.sh.askama",
        include_bytes!("../templates/overlay/prepare-launcher.sh.askama"),
    ),
    (
        "overlay/first-boot-launcher.sh.askama",
        include_bytes!("../templates/overlay/first-boot-launcher.sh.askama"),
    ),
    (
        "overlay/fstab.askama",
        include_bytes!("../templates/overlay/fstab.askama"),
    ),
    (
        "overlay/hostname.askama",
        include_bytes!("../templates/overlay/hostname.askama"),
    ),
    (
        "overlay/systemd/yaoshi-dashboard.service.askama",
        include_bytes!("../templates/overlay/systemd/yaoshi-dashboard.service.askama"),
    ),
    (
        "overlay/systemd/yaoshi-root-shell.service.askama",
        include_bytes!("../templates/overlay/systemd/yaoshi-root-shell.service.askama"),
    ),
    (
        "overlay/systemd/yaoshi-prepare.service.askama",
        include_bytes!("../templates/overlay/systemd/yaoshi-prepare.service.askama"),
    ),
    (
        "overlay/systemd/yaoshi-first-boot.service.askama",
        include_bytes!("../templates/overlay/systemd/yaoshi-first-boot.service.askama"),
    ),
    (
        "overlay/systemd/ssh-order.conf.askama",
        include_bytes!("../templates/overlay/systemd/ssh-order.conf.askama"),
    ),
    (
        "overlay/network/20-yaoshi-dhcp.network.askama",
        include_bytes!("../templates/overlay/network/20-yaoshi-dhcp.network.askama"),
    ),
    (
        "overlay/ssh/10-yaoshi.conf.askama",
        include_bytes!("../templates/overlay/ssh/10-yaoshi.conf.askama"),
    ),
    (
        "overlay/repart/20-yaoshi-root.conf.askama",
        include_bytes!("../templates/overlay/repart/20-yaoshi-root.conf.askama"),
    ),
];

const INSTALLED_ESP_SOURCES: [(&str, &[u8]); 6] = [
    (
        "esp/boot/KERNEL-RELEASE.askama",
        include_bytes!("../templates/esp/boot/KERNEL-RELEASE.askama"),
    ),
    (
        "esp/config/HOSTNAME.askama",
        include_bytes!("../templates/esp/config/HOSTNAME.askama"),
    ),
    (
        "esp/config/AUTHKEYS.askama",
        include_bytes!("../templates/esp/config/AUTHKEYS.askama"),
    ),
    (
        "esp/runtime/PREPARE.askama",
        include_bytes!("../templates/esp/runtime/PREPARE.askama"),
    ),
    (
        "esp/loader/loader.conf.askama",
        include_bytes!("../templates/esp/loader/loader.conf.askama"),
    ),
    (
        "esp/loader/yaoshi.conf.askama",
        include_bytes!("../templates/esp/loader/yaoshi.conf.askama"),
    ),
];

const INSTALLER_BOOT_SOURCES: [(&str, &[u8]); 3] = [
    (
        "installer-boot/boot/KERNEL-RELEASE.askama",
        include_bytes!("../templates/installer-boot/boot/KERNEL-RELEASE.askama"),
    ),
    (
        "installer-boot/loader/loader.conf.askama",
        include_bytes!("../templates/installer-boot/loader/loader.conf.askama"),
    ),
    (
        "installer-boot/loader/yaoshi-installer.conf.askama",
        include_bytes!("../templates/installer-boot/loader/yaoshi-installer.conf.askama"),
    ),
];
