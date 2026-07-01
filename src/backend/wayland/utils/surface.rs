use crate::backend::wayland::utils::shm::ShmBuffer;
use wayland_client::protocol::{wl_buffer, wl_region, wl_surface};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::Layer,
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
// imports over

pub struct SurfaceData {
    pub surface: wl_surface::WlSurface,
    pub layer_surface: ZwlrLayerSurfaceV1,
    pub shm_buffer: ShmBuffer,
    pub transparent_buffer: ShmBuffer,
    pub empty_region: wl_region::WlRegion,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    pub visibility: SurfaceVisibility,
}

#[derive(Debug)]
pub enum SurfaceVisibility {
    Visible,
    Hidden,
}

struct SurfaceVisibilityConfig<'a> {
    buffer: &'a wl_buffer::WlBuffer,
    layer: Layer,
    keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    input_region: Option<&'a wl_region::WlRegion>,
}

impl SurfaceData {
    pub fn set_visible(&mut self) {
        self.apply_visibility(SurfaceVisibility::Visible);
        self.visibility = SurfaceVisibility::Visible;
    }
    pub fn set_hidden(&mut self) {
        self.apply_visibility(SurfaceVisibility::Hidden);
        self.visibility = SurfaceVisibility::Hidden;
    }

    fn apply_visibility(&self, visibility: SurfaceVisibility) {
        let cfg = match visibility {
            SurfaceVisibility::Visible => SurfaceVisibilityConfig {
                buffer: &self.shm_buffer.buffer,
                layer: Layer::Overlay,
                keyboard: zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                input_region: None,
            },
            SurfaceVisibility::Hidden => SurfaceVisibilityConfig {
                buffer: &self.transparent_buffer.buffer,
                layer: Layer::Background,
                keyboard: zwlr_layer_surface_v1::KeyboardInteractivity::None,
                input_region: Some(&self.empty_region),
            },
        };

        self.layer_surface.set_keyboard_interactivity(cfg.keyboard);
        self.surface.attach(Some(cfg.buffer), 0, 0);
        self.surface
            .damage_buffer(0, 0, self.width as i32, self.height as i32);
        self.layer_surface.set_layer(cfg.layer);
        self.surface.set_input_region(cfg.input_region);
        self.surface.commit();
    }
}

//Since there isn't straightforward implementation of hiding
//screenshot and its editing overlay, the best solution i've found
//is to make it transparent, and:
//* set KeyboardInteractivity::None to not accept keyboard input
//* set layer to background, so technically it will be lower than everthing else
//* set input region - empty, even if its layer background, its still higher than user's desktop, so its necessary to not block mouse input

// deactivated                                                             //  activated
//     sd.layer_surface.set_keyboard_interactivity(                        //      sd.layer_surface.set_keyboard_interactivity(
//         zwlr_layer_surface_v1::KeyboardInteractivity::None,             //          zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
//     );                                                                  //      );
//     sd.surface.attach(Some(&sd.transparent_buffer.buffer), 0, 0);       //      sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
//     sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);  //      sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
//     sd.layer_surface.set_layer(Layer::Background);                      //      sd.layer_surface.set_layer(Layer::Overlay);
//     sd.surface.set_input_region(Some(&sd.empty_region));                //      sd.surface.set_input_region(None);
//     sd.surface.commit();                                                //      sd.surface.commit();
