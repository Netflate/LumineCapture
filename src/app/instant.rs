// --full and --window: take the shot and hand it over right away, no overlay and no editor

use crate::backend::{initialize_capture, initialize_overlay};
use crate::profiler::Profiler;
use crate::renderer;
use crate::utils::{encode_png, get_full_workspace_rect};

use super::{Launch, deliver, init};

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

pub async fn full(conn: wayland_client::Connection, launch: Launch) -> Result {
    let mut prof = Profiler::new(launch.speed);

    // 'overlay' may be misleading, we use get monitor list and probe of the active
    // witout actually showing the overlay
    let mut overlay = initialize_overlay(conn.clone())?;
    prof.mark("outputs discovered");
    let all = overlay.discovered_outputs().to_vec();
    let outputs = match launch.one_monitor.then(|| overlay.active_output()) {
        Some(Some(idx)) => vec![all[idx].clone()],
        Some(None) => {
            log::warn!("Can't tell which monitor is active, capturing all of them");
            all
        }
        None => all,
    };
    if launch.one_monitor {
        prof.mark("active output probed");
    }

    let shots = initialize_capture(&conn).capture_frame(&outputs).await?;
    drop(overlay);
    prof.mark("capture");

    let outputs: Vec<_> = shots
        .frames
        .iter()
        .map(|f| outputs[f.output].clone())
        .collect();
    let captures = init::build_captures(shots.frames)?;
    let placements = init::build_placements(&outputs);
    let region = get_full_workspace_rect(&placements).ok_or("no monitor was captured")?;
    let (pixmap, _, scale) = renderer::composite(&captures, &placements, region, None)
        .ok_or("couldn't put the monitors together")?;
    let png = encode_png(&pixmap);
    prof.mark("png encoded");

    deliver(png, scale, launch.outputs(), launch.output.as_deref()).await;
    prof.mark("delivered");
    prof.dump();
    Ok(())
}

pub async fn window(conn: wayland_client::Connection, launch: Launch) -> Result {
    let mut prof = Profiler::new(launch.speed);
    if launch.one_monitor {
        log::warn!("--monitor does nothing with --window, ignoring it");
    }

    let capture = initialize_capture(&conn).capture_active_window().await?;
    prof.mark("capture");
    let png = encode_png(&capture.pixmap);
    prof.mark("png encoded");

    deliver(
        png,
        capture.scale,
        launch.outputs(),
        launch.output.as_deref(),
    )
    .await;
    prof.mark("delivered");
    prof.dump();
    Ok(())
}
