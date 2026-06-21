use crate::backend::{ScreenOverlay, initialize_capture, initialize_clipboard, initialize_overlay};
use crate::types::{DamageRect, EditorState, MagnifierState, MonitorFrame, MouseButton, OverlayEvent, Placement, PointerState, SelectionEdges, SelectionState, HANDLE_RADIUS, MAG_FRAME_INTERVAL};
use crate::types::toolbar::{TOOLBAR_HEIGHT, TOOLBAR_OFFSET, TOOLBAR_PADDING, Toolbar, ToolbarAnimation, ToolbarAction, ToolbarButton, ToolbarItem, ToolbarSide};
use tiny_skia::{Pixmap, PixmapPaint, Transform, Rect};
use crate::renderer::{self};
use crate::utils::{global_point_to_local, encode_png, save_to_file, get_overlapping_monitors};
use std::time::{Instant};
use std::collections::HashMap;
use crate::tools::{dispatch_button, dispatch_move, Tool};
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use usvg::Tree;
use crate::types::icons;
use crate::types::annotations::Annotation;


pub async fn make_screenshot (
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let t0 = std::time::Instant::now();                               
    let icons_handle = std::thread::spawn(load_icons_cache);

    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let screenshots = capture.capture_frame().await?;
    
    println!("after capturing {}ms", t0.elapsed().as_millis());               
    let mut overlay = initialize_overlay(conn.clone());
    let clipboard = initialize_clipboard(conn);
    
    
    let base_pixmaps: Vec<Pixmap> = build_base_pixmap(&screenshots.frames);
    println!("after initialising base_pixmaps, which are original frames in Vec<Pixmap> {}ms", t0.elapsed().as_millis());                
    let (canvas, dimmed) = build_layers(&base_pixmaps);
    println!("after initialising dimmed canvas, same size dimmed frames {}ms", t0.elapsed().as_millis());                
    let placements = build_placements(&screenshots.frames); 


    drop(screenshots); 
    // 4 may 2026 : ~75mb memory usage while screenshoting on kde linux with 2 hd monitors
    // not ideal, could be resolved with rendering in shm itself 

    let icon_cache = icons_handle.join().expect("Failed to join icons thread");
    let mut editor_state = EditorState {
        base: base_pixmaps,
        canvas: canvas,
        dimmed : dimmed,
        selected_tool: Tool::Selection,
        tool_active: false,
        selection: SelectionState::default(),
        placements : placements,
        drag_start: None,
        pointer: PointerState::default(),
        magnifier: None,
        prev_magnifier: None,
        last_mag_update: None,
        mouse_down_left: false,
        toolbar: Toolbar::new(), 
        icon_cache,

        annotations: Vec::new(),
        pending: None,
        next_id: 0,
        prev_pending: None,
    };
    
    println!("after saving base screenshot {}ms", t0.elapsed().as_millis());                
    let outputs = overlay.present(&editor_state.placements)?.to_vec();
    
    initial_paint(&mut editor_state, &mut overlay)?;
    println!("after initialising overlay and showing it {}ms", t0.elapsed().as_millis());                


    let mut dirty_mask : u32 = 0 ;
    let mut selection_dirty = false;

    // todo: Remove hardcoded settings, will be saved later in a config-like structure 
    let mut save_to_clipboard = false;
    let _save_as_file = true;                     
    loop {
        // rendering happens only if x event happens
        // if we have some kind of animation we need to force rendering every 16 ms 
        // to create a 60 fps animation
        let opacity_animating = {
            let tb = &editor_state.toolbar;
            let target_opacity = if tb.interferes {0.1} else {1.0};
            (tb.opacity - target_opacity).abs() > 0.001
        };
        let timeout = if editor_state.toolbar.anim.is_some() || opacity_animating { 16 } else { -1 };

        let ev = overlay.next_event(timeout)?;
        match ev {
            OverlayEvent::Tick => {}
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { monitor_idx, x, y } => {
                handle_pointer_move(
                    &mut editor_state,
                    monitor_idx,
                    x,
                    y,
                    &mut dirty_mask,
                    &mut selection_dirty,
                );
            }
            
            OverlayEvent::PointerButton { button, pressed } => {
                handle_pointer_button(&mut editor_state, button, pressed, &mut dirty_mask);
            }
            OverlayEvent::SaveToClipboard => {
                drop(overlay);
                save_to_clipboard = true;
                break
            }
        }

        tick_toolbar_anim(&mut editor_state, &mut dirty_mask);

        if dirty_mask != 0 {
            for i in 0..outputs.len() {
                if is_dirty(dirty_mask, i) {                    
                    let is_mag_monitor = editor_state.magnifier
                        .as_ref()
                        .map_or(false, |m| m.monitor_idx == i);

                    let (local_sel, prev_local, edges) = selection_render_info(
                        &editor_state.selection.zone,
                        &editor_state.selection.prev_zone,
                        &editor_state.placements[i],
                    );
                    let dirty_rect = editor_state.monitor_dirty_rect(i, selection_dirty);
                    let damage: Option<DamageRect> = dirty_rect
                        .as_ref()
                        .and_then(|r| renderer::rect_bounds(r, editor_state.base[i].width(), editor_state.base[i].height()));

                    if i == editor_state.toolbar.monitor_idx && !editor_state.toolbar.dirty {
                        if let Some(dirty) = dirty_rect.as_ref() {
                            let tb = &editor_state.toolbar;
                            let tb_rect = Rect::from_xywh(tb.position.0, tb.position.1, tb.size.0, TOOLBAR_HEIGHT);
                            if let Some(tb_r) = tb_rect {
                                let intersects = dirty.left() < tb_r.right() && dirty.right() > tb_r.left()
                                    && dirty.top() < tb_r.bottom() && dirty.bottom() > tb_r.top();
                                if intersects {
                                    editor_state.toolbar.dirty = true;
                                }
                            }
                        }
                    }

                    let toolbar = if i == editor_state.toolbar.monitor_idx && editor_state.toolbar.dirty {
                        Some(&mut editor_state.toolbar)
                    } else {
                        None
                    };

                    let offset = (
                        editor_state.placements[i].position.0 as f32,
                        editor_state.placements[i].position.1 as f32,
                    );
                    let local_annotations: Vec<Annotation> = editor_state.annotations
                        .iter()
                        .map(|a| a.to_local(offset))
                        .collect();
                    let local_pending = editor_state.pending
                        .as_ref()
                        .map(|a| a.to_local(offset));


                    renderer::render_frame(&mut renderer::RenderRequest {
                        canvas: &mut editor_state.canvas[i],
                        base: &editor_state.base[i],
                        dimmed: &mut editor_state.dimmed[i],
                        selection: local_sel.as_ref(),
                        prev_selection: prev_local.as_ref(),
                        dirty_rect: dirty_rect.as_ref(),
                        selection_edges: edges.as_ref(),
                        selection_dirty,
                        magnifier: editor_state.magnifier.as_ref(),
                        is_mag_monitor,
                        toolbar,
                        icons_cache: &editor_state.icon_cache,

                        annotations: &local_annotations,
                        pending: local_pending.as_ref(),
                    });

                    overlay.update_frame(i, editor_state.canvas[i].data(), damage)?;

                }
            }
            selection_dirty = false;         
            editor_state.toolbar.dirty = false;
            dirty_mask = 0;
            editor_state.prev_pending = editor_state.pending.clone();
        }
    }
    if save_to_clipboard {
        let final_result = render_final(&editor_state);
        // it doesn't make sense, but while this program in wip 
        // it will have one option - save to clipboard AND file

        let _path = save_to_file(&final_result);
        clipboard.copy_image_to_clipboard(final_result)?;   
    }

    Ok(())
}





fn build_base_pixmap(frames: &Vec<MonitorFrame>) -> Vec<Pixmap> {
    frames
        .iter()
        .enumerate()
        .map(|(monitor_idx, f)| {
            let (src_w, src_h) = (f.pw_width, f.pw_height);
            let mut src_pixmap = Pixmap::new(src_w, src_h)
                .expect("Failed to create source Pixmap for monitor");

            let row_bytes = (src_w as usize) * 4;
            let src_stride = f.pw_stride as usize;
            let dst = src_pixmap.data_mut();

            if src_stride < row_bytes {
                panic!(
                    "Invalid stride for monitor {}: stride={} row_bytes={}",
                    monitor_idx, src_stride, row_bytes
                );
            }

            let needed = src_stride * (src_h as usize);
            let src = f
                .pixels
                .get(..needed)
                .unwrap_or_else(|| panic!(
                    "Not enough pixel data for monitor {}: have={} need={}",
                    monitor_idx,
                    f.pixels.len(),
                    needed
                ));

            for row in 0..(src_h as usize) {
                let src_off = row * src_stride;
                let dst_off = row * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }

            let (logical_w_i32, logical_h_i32) = f
                .info
                .size
                .unwrap_or((src_w as i32, src_h as i32));
            let logical_w = logical_w_i32.max(1) as u32;
            let logical_h = logical_h_i32.max(1) as u32;

            if logical_w == src_w && logical_h == src_h {
                return src_pixmap;
            }

            let mut logical_pixmap = Pixmap::new(logical_w, logical_h)
                .expect("Failed to create logical Pixmap for monitor");
            let sx = logical_w as f32 / src_w as f32;
            let sy = logical_h as f32 / src_h as f32;
            logical_pixmap.draw_pixmap(
                0,
                0,
                src_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::from_row(sx, 0.0, 0.0, sy, 0.0, 0.0),
                None,
            );
            logical_pixmap
        })
        .collect()
}

fn build_layers(base_pixmaps: &[Pixmap]) -> (Vec<Pixmap>, Vec<Pixmap>) {
    base_pixmaps
        .iter()
        .map(|p| {
            let w = p.width();
            let h = p.height();

            let canvas = Pixmap::new(w, h).expect("Failed to create canvas Pixmap");
            let dimmed = Pixmap::new(w, h).expect("Failed to create dimmed Pixmap");

            (canvas, dimmed)
        })
        .unzip() 
}



fn build_placements(frames: &Vec<MonitorFrame>) -> Vec<Placement> {
    frames.iter()
    .map(|stream| Placement {
        position: stream.info.position.unwrap_or((0, 0)),
        size: stream
            .info
            .size
            .unwrap_or((stream.pw_width as i32, stream.pw_height as i32)),
    })
    .collect()
}


fn initial_paint(
    editor_state: &mut EditorState,
    overlay: &mut Box<dyn ScreenOverlay>,
) -> Result<(), Box<dyn std::error::Error>>
{
    for monitor_idx in 0..editor_state.base.len() {
        let (local_sel, prev_local, edges) = selection_render_info(
            &editor_state.selection.zone,
            &editor_state.selection.prev_zone,
            &editor_state.placements[monitor_idx],
        );
        renderer::init_dimming(
            &mut editor_state.dimmed[monitor_idx],
            &editor_state.base[monitor_idx],
            &local_sel,
        );
        renderer::render_frame(&mut renderer::RenderRequest {
            canvas: &mut editor_state.canvas[monitor_idx],
            base: &editor_state.base[monitor_idx],
            dimmed: &mut editor_state.dimmed[monitor_idx],
            selection: local_sel.as_ref(),
            prev_selection: prev_local.as_ref(),
            dirty_rect: None,
            selection_edges: edges.as_ref(),
            selection_dirty: false,
            magnifier: editor_state.magnifier.as_ref(),
            is_mag_monitor: false,
            toolbar: None,
            icons_cache: &editor_state.icon_cache,

            annotations: &editor_state.annotations,
            pending: None,
        });
        overlay.update_frame(monitor_idx, editor_state.canvas[monitor_idx].data(), None)?;
    }

    Ok(())
}



fn handle_pointer_move(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    x: f64,
    y: f64,
    dirty_mask: &mut u32,
    selection_dirty: &mut bool,
) {
    let global = (
        editor_state.placements[monitor_idx].position.0 as f64 + x,
        editor_state.placements[monitor_idx].position.1 as f64 + y,
    );

    let (current_monitor_idx, local_x, local_y) = global_point_to_local(
        &editor_state.placements, global, monitor_idx,(x, y)
    );
    update_pointer(editor_state, current_monitor_idx, (local_x, local_y), global);


    update_magnifier(editor_state, dirty_mask);
    dispatch_move(editor_state.selected_tool, editor_state, global, selection_dirty, dirty_mask);
    update_toolbar(editor_state, dirty_mask);
}

fn update_pointer(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    global: (f64, f64),
) {
    editor_state.pointer = PointerState::new(monitor_idx, local, global);
}

fn update_magnifier(
    editor_state: &mut EditorState,
    dirty_mask: &mut u32,
) {
    let now = Instant::now();
    if let Some(last) = editor_state.last_mag_update {
        if now.duration_since(last) < MAG_FRAME_INTERVAL {
            return;
        }
    }
    editor_state.last_mag_update = Some(now);

    let monitor_idx = editor_state.pointer.monitor_idx;
    let local = editor_state.pointer.local;


    if let Some(mag) = editor_state.magnifier.as_ref() {
        if mag.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, mag.monitor_idx);
        }
        editor_state.prev_magnifier = Some(MagnifierState {
            monitor_idx: mag.monitor_idx,
            pos: mag.pos,
        });
    } else {
        editor_state.prev_magnifier = None;
    }

    editor_state.magnifier = Some(MagnifierState {
        monitor_idx,
        pos: local,
    });

    mark_dirty(dirty_mask, monitor_idx);
}


pub fn mark_dirty(mask: &mut u32, idx: usize) {
    *mask |= 1 << idx;
}


fn is_dirty(mask: u32, idx: usize) -> bool {
    (mask & (1 << idx)) != 0
}

fn selection_render_info(
    selection: &Option<Rect>,
    prev_selection: &Option<Rect>,
    placement: &Placement,
) -> (Option<Rect>, Option<Rect>, Option<SelectionEdges>) {
    let local_sel = selection
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement));
    let prev_local = prev_selection
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement));

    let mut edges = None;
    // let mut handles = Vec::new();

    if let (Some(sel), Some(_)) = (selection.as_ref(), local_sel.as_ref()) {
        edges = Some(selection_edges_for_monitor(sel, placement));
        // let (mx, my) = (placement.position.0 as f32, placement.position.1 as f32);
        // for (hx, hy) in selection_handle_points(sel) {
        //     if point_in_monitor((hx, hy), placement) {
        //         handles.push((hx - mx, hy - my));
        //     }
        // }
    }

    (local_sel, prev_local, edges)
}

fn expand_rect(rect: &Rect, pad: f32) -> Option<Rect> {
    Rect::from_ltrb(
        rect.left() - pad,
        rect.top() - pad,
        rect.right() + pad,
        rect.bottom() + pad,
    )
}

fn union_rect(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(r1), Some(r2)) => Rect::from_ltrb(
            r1.left().min(r2.left()),
            r1.top().min(r2.top()),
            r1.right().max(r2.right()),
            r1.bottom().max(r2.bottom()),
        ),
    }
}


fn render_final(editor_state: &EditorState) -> Vec<u8> {
    let sel = match editor_state.selection.zone {
        Some(s) => s,
        None => return vec![],
    };

    // based on selection choosing monitors for render
    let mask = get_overlapping_monitors(&sel, &editor_state.placements);

    let sel_left = sel.left().floor() as i32;
    let sel_top = sel.top().floor() as i32;
    let sel_right = sel.right().ceil() as i32;
    let sel_bottom = sel.bottom().ceil() as i32;
    let sel_w = (sel_right - sel_left).max(0) as u32;
    let sel_h = (sel_bottom - sel_top).max(0) as u32;
    if sel_w == 0 || sel_h == 0 {
        return vec![];
    }

    let mut out = Pixmap::new(sel_w, sel_h).unwrap();
    for (i, placement) in editor_state.placements.iter().enumerate() {
        if (mask & (1 << i)) == 0 { continue; }

        let dst_x = placement.position.0 - sel_left;
        let dst_y = placement.position.1 - sel_top;

        out.draw_pixmap(
            dst_x,
            dst_y,
            editor_state.base[i].as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    // re-drawing annotations
    // maybe in the future it would be better to make a separate layer 
    // instead of redrawing them
    // or for some specific perfomance eating tools as blur
    let offset = (sel_left as f32, sel_top as f32);
    for ann in &editor_state.annotations {
        let local = ann.to_local(offset);
        renderer::draw_annotation(&mut out, &local);
    }

    encode_png(&out)
}


impl EditorState {
    // for maximal optimization we render only a specific zone of the screen 
    // instead of entire screen 
    // entire function take what have changed, and add to dirty rectangle what need to be deleted
    // and what need to be added 
    pub fn monitor_dirty_rect(&self,monitor_idx: usize,selection_dirty: bool) -> Option<Rect> {
    // Selection part 
    let mut dirty: Option<Rect> = None;
    let placement = &self.placements[monitor_idx];
    if selection_dirty {
        let selection_pad = (HANDLE_RADIUS as f32).max(4.0);
        
        let local_sel = self.selection.zone
            .as_ref()
            .and_then(|sel| global_selection_to_local(sel, placement));
            
        let prev_local = self.selection.prev_zone
            .as_ref()
            .and_then(|sel| global_selection_to_local(sel, placement));

        if let Some(r) = local_sel.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
        if let Some(r) = prev_local.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
    }
    // Magnifier part 
    let (mw, mh) = (placement.size.0 as f32, placement.size.1 as f32);
    if mw > 0.0 && mh > 0.0 {
        let mag_pad = 2.0;
        
        let mut add_mag_dirty = |mag_state: &Option<MagnifierState>| {
            if let Some(mag) = mag_state.as_ref().filter(|m| m.monitor_idx == monitor_idx) {
                let rect = renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
                if let Some(r) = expand_rect(&rect, mag_pad) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        };

        add_mag_dirty(&self.magnifier);
        add_mag_dirty(&self.prev_magnifier);
    }
    // Toolbar
    let tb = &self.toolbar;
    if tb.dirty {
        if tb.monitor_idx == monitor_idx {
            if let Some(r) = Rect::from_xywh(tb.position.0, tb.render_y, tb.size.0, TOOLBAR_HEIGHT) {
                dirty = union_rect(dirty, Some(r));
            }

            // if toolbar position (side) changed
            if tb.prev_monitor_idx == monitor_idx && tb.prev_position != tb.position {
                if let Some(r) = Rect::from_xywh(tb.prev_position.0, tb.prev_position.1, tb.size.0, TOOLBAR_HEIGHT) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        }
        // if toolbar monitor changed
        if tb.prev_monitor_idx == monitor_idx && tb.prev_monitor_idx != tb.monitor_idx {
            if let Some(r) = Rect::from_xywh(tb.prev_position.0, tb.prev_position.1, tb.size.0, TOOLBAR_HEIGHT) {
                dirty = union_rect(dirty, Some(r));
            }
        }
    }
    // Annotations
    let offset = (placement.position.0 as f32, placement.position.1 as f32);
    let pad = 4.0;

    let mut add_ann_dirty = |ann: &Annotation| {
        if let Some(bbox) = ann.bounding_box() {
            if let Some(local) = Rect::from_ltrb(
                bbox.left()   - offset.0,
                bbox.top()    - offset.1,
                bbox.right()  - offset.0,
                bbox.bottom() - offset.1,
            ) {
                if let Some(r) = expand_rect(&local, pad) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        }
    };

    if let Some(ann) = &self.pending {
        add_ann_dirty(ann);
    }
    if let Some(ann) = &self.prev_pending {
        add_ann_dirty(ann);
    }
    dirty
    }
}


fn handle_pointer_button(
    editor_state: &mut EditorState,
    button: MouseButton,
    pressed: bool,
    dirty_mask: &mut u32,
) {
    if matches!(button, MouseButton::Left) && pressed {
        if let Some(tb_button) = toolbar_hit_test(&editor_state.toolbar, editor_state.pointer.local) {
            editor_state.toolbar.dirty = true;
            mark_dirty(dirty_mask, editor_state.toolbar.monitor_idx);

            if let Some(ToolbarItem::Button(button)) = editor_state.toolbar.items.get(tb_button) {
                match button {
                    ToolbarButton::Tool(tool) => {
                        editor_state.selected_tool = *tool;
                        editor_state.toolbar.selected = Some(tb_button);
                    }
                    ToolbarButton::Action(ToolbarAction::SideChange) => {
                        editor_state.toolbar.current_side = match editor_state.toolbar.current_side {
                            ToolbarSide::Top => ToolbarSide::Bottom,
                            ToolbarSide::Bottom => ToolbarSide::Top,
                        };
                    }
                }
                editor_state.toolbar.dirty = true;
                update_toolbar(editor_state, dirty_mask);
            }
            return;
        }
    }

    dispatch_button(editor_state.selected_tool, editor_state, button, pressed, dirty_mask);
    
    if matches!(button, MouseButton::Left) && !pressed {
        update_toolbar(editor_state, dirty_mask);
    }
}


// Toolbar section 
fn update_toolbar(
    editor_state: &mut EditorState, 
    dirty_mask: &mut u32
) {
    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let (pos_x, pos_y) = toolbar_position(
        editor_state.toolbar.current_side, 
        placement, 
        editor_state.toolbar.toolbar_width()
    );
    let from_y = match editor_state.toolbar.current_side {
        ToolbarSide::Top    => -TOOLBAR_HEIGHT,
        ToolbarSide::Bottom => placement.size.1 as f32,
    };
    let interferes = toolbar_interferes(editor_state);


    let pointer_local = editor_state.pointer.local;

    let tb = &mut editor_state.toolbar;
    if tb.interferes != interferes {
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }
    tb.interferes = interferes;

    if tb.monitor_idx != monitor_idx || tb.position != (pos_x, pos_y) {
        tb.anim = Some(ToolbarAnimation {
            start: Instant::now(),
            duration_ms: 200,
            from_y,
            to_y: pos_y,
        });
        tb.render_y = from_y;
        tb.prev_position = tb.position;
        tb.prev_monitor_idx = tb.monitor_idx;
        mark_dirty(dirty_mask, tb.monitor_idx);
        if tb.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, monitor_idx);
        }
        tb.dirty = true;
        tb.position = (pos_x, pos_y);
        tb.monitor_idx = monitor_idx;
    }

    let button = toolbar_hit_test(tb, pointer_local);
    if button != tb.hovered {
        tb.hovered = button;
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }
}

fn toolbar_interferes(editor_state: &EditorState) -> bool {
    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let mon_w = placement.size.0 as f32;
    let mon_h = placement.size.1 as f32;
    let tb = &editor_state.toolbar;
    let tb_width = tb.size.0;
    let x = (mon_w - tb_width) / 2.0;

    let active_rect = match tb.current_side {
        ToolbarSide::Top    => (x, TOOLBAR_OFFSET, x + tb_width, TOOLBAR_OFFSET + TOOLBAR_HEIGHT),
        ToolbarSide::Bottom => (x, mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT, x + tb_width, mon_h - TOOLBAR_OFFSET),
    };

    let Some(sel) = editor_state.selection.zone
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement))
    else { return false };

    active_rect.0 < sel.right()  && active_rect.2 > sel.left() &&
    active_rect.1 < sel.bottom() && active_rect.3 > sel.top()  && 
    editor_state.tool_active                                      // if selection isn't active toolbar won't be transparent 
}

fn toolbar_position(side: ToolbarSide, placement: &Placement, tb_width: f32) -> (f32, f32) {
    let mon_w = placement.size.0 as f32;
    let mon_h = placement.size.1 as f32;
    let x = (mon_w - tb_width) / 2.0;
    let y = match side {
        ToolbarSide::Top    => TOOLBAR_OFFSET,
        ToolbarSide::Bottom => mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT,
    };
    (x, y)
}

fn load_icons_cache() -> HashMap<ToolbarButton, Tree> {
    let t0 = std::time::Instant::now();
    let mut cache = HashMap::new();
    let opt = usvg::Options::default();

    for item in crate::types::toolbar::TOOLBAR_ITEMS {
        let ToolbarItem::Button(button) = item else { continue };
        let (svg_str, _) = icons::get_svg(button);
        
        let tree = Tree::from_str(svg_str, &opt)
            .expect("Critical: Failed to parse embedded SVG icon");
        
        cache.insert(*button, tree);
    }
    println!("(Background thread) : svg parsed in {}ms ", t0.elapsed().as_millis());

    cache
}


pub fn toolbar_hit_test(toolbar: &Toolbar, local: (f64, f64)) -> Option<usize> {
    let (px, py) = (local.0 as f32, local.1 as f32);
    let (tb_x, tb_y) = toolbar.position;
    let (_, tb_h) = toolbar.size;

    if py < tb_y || py > tb_y + tb_h {
        return None;
    }

    let mut current_x = tb_x + TOOLBAR_PADDING;

    for (idx, item) in toolbar.items.iter().enumerate() {
        let item_w = item.size();
        let item_right = current_x + item_w;

        if px >= current_x && px <= item_right {
            return match item {
                ToolbarItem::Button(_) => Some(idx),
                ToolbarItem::Seperator => None, 
            };
        }

        current_x += item_w + item.trailing_padding();
    }

    None
}


fn tick_toolbar_anim(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let tb = &mut editor_state.toolbar;
    
    // smooth opacity animation 
    let now = Instant::now();
    let dt = tb.last_tick
        .map(|t| now.duration_since(t).as_secs_f32())
        .unwrap_or(0.016);
    tb.last_tick = Some(now);

    let opacity_per_sec = 5.0; 
    let target_opacity = if tb.interferes { 0.1 } else { 1.0 };
    if (tb.opacity - target_opacity).abs() > 0.001 {
        let delta = opacity_per_sec * dt;
        if tb.opacity < target_opacity {
            tb.opacity = (tb.opacity + delta).min(target_opacity);
        } else {
            tb.opacity = (tb.opacity - delta).max(target_opacity);
        }
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }

    // ease out animation
    let Some(anim) = &tb.anim else { return };

    let t = (anim.start.elapsed().as_millis() as f32 / anim.duration_ms as f32).clamp(0.0, 1.0);
    let t_eased = 1.0 - (1.0 - t).powi(3); 

    tb.render_y = anim.from_y + (anim.to_y - anim.from_y) * t_eased;
    tb.dirty = true;
    mark_dirty(dirty_mask, tb.monitor_idx);

    if t >= 1.0 {
        tb.render_y = anim.to_y;
        tb.anim = None;
    }
}

