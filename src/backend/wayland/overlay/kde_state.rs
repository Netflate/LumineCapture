use wayland_protocols_plasma::plasma_virtual_desktop::client::org_kde_plasma_virtual_desktop_management::OrgKdePlasmaVirtualDesktopManagement;

pub struct KdeState {
    pub virtual_desktop_manager: Option<OrgKdePlasmaVirtualDesktopManagement>,
    pub current_desktop: Option<String>,
    pub pending_desktop_ids: Vec<String>,
}