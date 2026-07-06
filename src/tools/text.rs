use crate::tools::ToolBehavior;
use crate::types::{MouseButton, Annotation, AnnotationShape, TextEditState, SpecialKey};
use crate::editor::EditorState;
use crate::types::annotations::{begin_drag_for_annotation, apply_annotation_drag, commit_drag_if_changed};

use tiny_skia::{Rect, Color, PixmapMut};
use cosmic_text::{
    Action, Attrs, Buffer, Edit, Editor, Family, Metrics,
    Motion, Selection, Shaping, SwashCache,
};

pub struct TextTool;

// ── key → Action ─────────────────────────────────────────────────────────────

fn key_to_action(key: SpecialKey, ctrl: bool) -> Option<Action> {
    match key {
        SpecialKey::KeyA      => None,
        SpecialKey::Left      => Some(Action::Motion(if ctrl { Motion::LeftWord    } else { Motion::Left        })),
        SpecialKey::Right     => Some(Action::Motion(if ctrl { Motion::RightWord   } else { Motion::Right       })),
        SpecialKey::Home      => Some(Action::Motion(if ctrl { Motion::BufferStart } else { Motion::Home        })),
        SpecialKey::End       => Some(Action::Motion(if ctrl { Motion::BufferEnd   } else { Motion::End         })),
        SpecialKey::Up        => Some(Action::Motion(Motion::Up)),
        SpecialKey::Down      => Some(Action::Motion(Motion::Down)),
        SpecialKey::Backspace => if ctrl { None } else { Some(Action::Backspace) },
        SpecialKey::Delete    => if ctrl { None } else { Some(Action::Delete)    },
        SpecialKey::Enter     => Some(Action::Enter),
    }
}

pub fn apply_key_to_editor(
    editor: &mut Editor<'static>,
    font_system: &mut cosmic_text::FontSystem,
    key: SpecialKey,
    ctrl: bool,
    shift: bool,
) -> bool {

    if ctrl && matches!(key, SpecialKey::KeyA) {
        let start_cursor = cosmic_text::Cursor::new(0, 0);
        editor.set_selection(Selection::Normal(start_cursor));
        editor.action(font_system, Action::Motion(Motion::BufferEnd));
        return false; 
    }

    let is_motion = matches!(
        key,
        SpecialKey::Left | SpecialKey::Right
            | SpecialKey::Home | SpecialKey::End
            | SpecialKey::Up   | SpecialKey::Down
    );

    if is_motion {
        if shift {
            // Shift + Motion
            if editor.selection_bounds().is_none() {
                editor.set_selection(Selection::Normal(editor.cursor()));
            }
        } else {
            // Motion without Shift
            editor.set_selection(Selection::None);
        }
    }

    // Ctrl+Backspace
    if ctrl && matches!(key, SpecialKey::Backspace) {
        if editor.selection_bounds().is_none() {
            editor.set_selection(Selection::Normal(editor.cursor()));
        }
        editor.action(font_system, Action::Motion(Motion::LeftWord));
        editor.delete_selection();
        return true;
    }

    // Ctrl+Delete
    if ctrl && matches!(key, SpecialKey::Delete) {
        if editor.selection_bounds().is_none() {
            editor.set_selection(Selection::Normal(editor.cursor()));
        }
        editor.action(font_system, Action::Motion(Motion::RightWord));
        editor.delete_selection();
        return true;
    }

    if let Some(action) = key_to_action(key, ctrl) {
        let modifies = matches!(action, Action::Backspace | Action::Delete | Action::Enter);
        editor.action(font_system, action);
        return modifies;
    }

    false
}

// ── ToolBehavior ─────────────────────────────────────────────────────────────

impl ToolBehavior for TextTool {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, _dirty_mask: &mut u32) {
        if !matches!(button, MouseButton::Left) { return; }

        if !pressed {
            commit_drag_if_changed(state);
            return;
        }

        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);

        // 1. possibility to select other text fields (replacing tools/pick.rs)
        for (i, ann) in state.annotations.iter().enumerate().rev() {
            if !matches!(ann.shape, AnnotationShape::Text { .. }) { continue; }
            if !ann.initial_hit_test(state.pointer.global) { continue; }

            if let Some(prev) = state.text_editing.as_ref() {
                if prev.annotation_id != ann.id {
                    if let Some(prev_ann) = state.annotations.iter().find(|a| a.id == prev.annotation_id) {
                        state.damage_rects.push(prev_ann.damage_bbox(true));
                    }
                    if let Some(prev_editor) = state.text_editors.get_mut(&prev.annotation_id) {
                        prev_editor.set_selection(Selection::None);
                    }
                }
            }

            state.damage_rects.push(ann.damage_bbox(true));

            let ann_id = ann.id;
            if let Some(editor) = state.text_editors.get_mut(&ann_id) {
                editor.set_selection(Selection::None);
                editor.action(&mut state.font_system, Action::Motion(Motion::BufferEnd));
            }

            state.text_editing = Some(TextEditState { annotation_id: ann_id, cursor: 0 });
            state.selected_annotation = Some(i);
            state.annotations_dirty = true;
            begin_drag_for_annotation(state, i);
            return;
        }

        // 2. clicking on empty > remove prev text focus if exists
        if let Some(edit) = state.text_editing.take() {
            if let Some(prev_ann) = state.annotations.iter().find(|a| a.id == edit.annotation_id) {
                state.damage_rects.push(prev_ann.damage_bbox(true));
            }
            if let Some(editor) = state.text_editors.get_mut(&edit.annotation_id) {
                editor.set_selection(Selection::None);
            }
            state.selected_annotation = None;
            state.annotations_dirty = true;
            return;
        }

        // 3. creating new text field
        let id = state.next_id;
        state.next_id += 1;

        let font_size = 24.0_f32;
        let metrics = Metrics::new(font_size, font_size * 1.2);

        let mut buffer = Buffer::new_empty(metrics);
        buffer.set_size(None, None);
        buffer.set_text(
            "",
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );

        let editor = Editor::new(buffer);
        state.text_editors.insert(id, editor);

        let mut ann = Annotation {
            id,
            shape: AnnotationShape::Text {
                start: pos,
                content: String::new(),
                font_size,
            },
            color: Color::from_rgba8(255, 255, 255, 255),
            stroke_width: 0.0,
            bbox: Rect::from_xywh(pos.0, pos.1, 10.0, metrics.line_height).unwrap(),
        };

        update_text_bbox_inline(
            &mut ann,
            state.text_editors.get_mut(&id).unwrap(),
            &mut state.font_system,
        );

        state.push_undo();
        state.damage_rects.push(ann.damage_bbox(true));
        state.annotations.push(ann);

        state.text_editing = Some(TextEditState { annotation_id: id, cursor: 0 });
        state.selected_annotation = Some(state.annotations.len() - 1);
        state.annotations_dirty = true;
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _sel_dirty: &mut bool, _dirty_mask: &mut u32) {
        apply_annotation_drag(state, global);
    }

    fn on_text(&self, state: &mut EditorState, ch: char, _dirty_mask: &mut u32) {
        let Some(edit) = state.text_editing.as_ref() else { return };
        let id = edit.annotation_id;

        if let Some(ann) = state.annotations.iter().find(|a| a.id == id) {
            state.damage_rects.push(ann.damage_bbox(true));
        }

        if let Some(editor) = state.text_editors.get_mut(&id) {
            editor.action(&mut state.font_system, Action::Insert(ch));
            sync_content_from_editor(id, editor, &mut state.annotations, &mut state.font_system);
        }

        if let Some(ann) = state.annotations.iter().find(|a| a.id == id) {
            state.damage_rects.push(ann.damage_bbox(true));
        }

        state.annotations_dirty = true;
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, _dirty_mask: &mut u32) {
        let Some(edit) = state.text_editing.as_ref() else { return };
        let id = edit.annotation_id;
        let ctrl = state.mod_ctrl;
        let shift = state.mod_shift;

        if let Some(ann) = state.annotations.iter().find(|a| a.id == id) {
            state.damage_rects.push(ann.damage_bbox(true));
        }

        if let Some(editor) = state.text_editors.get_mut(&id) {
            let text_changed = apply_key_to_editor(editor, &mut state.font_system, key, ctrl, shift);
            if text_changed {
                sync_content_from_editor(id, editor, &mut state.annotations, &mut state.font_system);
            }
        }

        if let Some(ann) = state.annotations.iter().find(|a| a.id == id) {
            state.damage_rects.push(ann.damage_bbox(true));
        }

        state.annotations_dirty = true;
    }

    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        if let Some(edit) = state.text_editing.take() {
            if let Some(ann) = state.annotations.iter().find(|a| a.id == edit.annotation_id) {
                state.damage_rects.push(ann.damage_bbox(true));
            }
            if let Some(editor) = state.text_editors.get_mut(&edit.annotation_id) {
                editor.set_selection(Selection::None);
            }
        }
        state.text_editing = None;
        state.selected_annotation = None;
        state.annotations_dirty = true;
    }
}

// ── Editor -> Annotation ────────────────────────────────────────

/// Used after any text changement
fn sync_content_from_editor(
    id: u64,    editor: &mut Editor<'static>,
    annotations: &mut Vec<Annotation>,
    font_system: &mut cosmic_text::FontSystem,
) {
    editor.shape_as_needed(font_system, false);

    let new_content: String = editor.with_buffer(|buf| {
        buf.lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                if i + 1 < buf.lines.len() {
                    format!("{}\n", line.text())
                } else {
                    line.text().to_string()
                }
            })
            .collect()
    });

    let Some(ann) = annotations.iter_mut().find(|a| a.id == id) else { return };
    let AnnotationShape::Text { content, start, font_size } = &mut ann.shape else { return };

    *content = new_content;

    let (x, y) = *start;
    let fallback_h = *font_size * 1.2;

    let (w, h) = editor.with_buffer(|buf| {
        let lh = buf.metrics().line_height;
        let mut max_w = 0.0_f32;
        let mut total_h = 0.0_f32;
        for run in buf.layout_runs() {
            if run.line_w > max_w { max_w = run.line_w; }
            total_h = run.line_y + lh;
        }
        (max_w.max(10.0), if total_h > 0.0 { total_h } else { lh })
    });

    ann.bbox = Rect::from_xywh(x, y, w, h)
        .unwrap_or_else(|| Rect::from_xywh(x, y, 10.0, fallback_h).unwrap());
}

// ── pub helpers ─────────────────────────────────────────────────────────

/// Since it needs editor its better to make this function seperate 
/// instead of using update_bbox from types/nnotations.rs
pub fn update_text_bbox_inline(
    ann: &mut Annotation,
    editor: &mut Editor<'static>,
    font_system: &mut cosmic_text::FontSystem,
) {
    let AnnotationShape::Text { start, font_size, .. } = &ann.shape else { return };
    let (x, y) = *start;
    let current_font_size = *font_size;
    let fallback_h = current_font_size * 1.2;

    let new_metrics = Metrics::new(current_font_size, current_font_size * 1.2);

    editor.with_buffer_mut(|buf| {
        if buf.metrics() != new_metrics {
            buf.set_metrics(new_metrics);
        }
    });

    editor.shape_as_needed(font_system, false);

    let (w, h) = editor.with_buffer(|buf| {
        let lh = buf.metrics().line_height;
        let mut max_w = 0.0_f32;
        let mut total_h = 0.0_f32;
        for run in buf.layout_runs() {
            if run.line_w > max_w { max_w = run.line_w; }
            total_h = run.line_y + lh;
        }
        (max_w.max(10.0), if total_h > 0.0 { total_h } else { lh })
    });

    ann.bbox = Rect::from_xywh(x, y, w, h)
        .unwrap_or_else(|| Rect::from_xywh(x, y, 10.0, fallback_h).unwrap());
}

/// Since renders requires editor and stuff
/// its better to leave it here
/// (used in renderer/annotations.rs)
pub fn render_text_annotation(
    ann: &Annotation,
    editor: &mut Editor<'static>,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut PixmapMut<'_>,
    offset: (f32, f32),
    is_editing: bool,
) {
    let AnnotationShape::Text { start, .. } = &ann.shape else { return };
    let ann_x = start.0 - offset.0;
    let ann_y = start.1 - offset.1;

    let r = (ann.color.red()   * 255.0) as u8;
    let g = (ann.color.green() * 255.0) as u8;
    let b = (ann.color.blue()  * 255.0) as u8;
    let a = (ann.color.alpha() * 255.0) as u8;

    let text_color     = cosmic_text::Color::rgba(r, g, b, a);
    let cursor_color   = cosmic_text::Color::rgba(255, 255, 255, 220);
    let sel_color      = cosmic_text::Color::rgba(100, 150, 255, 160);
    let sel_text_color = cosmic_text::Color::rgba(255, 255, 255, 255);
    let transparent    = cosmic_text::Color::rgba(0, 0, 0, 0);

    let (cur_col, sel_col) = if is_editing {
        (cursor_color, sel_color)
    } else {
        (transparent, transparent)
    };

    editor.shape_as_needed(font_system, false);

    let p_width = pixmap.width() as i32;
    let p_height = pixmap.height() as i32;
    let pixels = pixmap.pixels_mut();

    editor.draw(
        font_system,
        swash_cache,
        text_color,
        cur_col,
        sel_col,
        sel_text_color,
        |x, y, w, h, color| {
            let mut draw_w = w as f32;

            if color == sel_col && draw_w <= 1.0 {
                draw_w = (ann.bbox.width() - x as f32).max(10.0);
            }

            let start_x = (ann_x + x as f32).round() as i32;
            let start_y = (ann_y + y as f32).round() as i32;
            let w_int = draw_w.round() as i32;
            let h_int = h as i32;

            let src_a = color.a() as u32;
            if src_a == 0 { return; }

            let src_r = (color.r() as u32 * src_a) / 255;
            let src_g = (color.g() as u32 * src_a) / 255;
            let src_b = (color.b() as u32 * src_a) / 255;
            let inv_a = 255 - src_a;

            for dy in 0..h_int {
                let py = start_y + dy;
                if py < 0 || py >= p_height { continue; }

                for dx in 0..w_int {
                    let px = start_x + dx;
                    if px < 0 || px >= p_width { continue; }

                    let idx = (py * p_width + px) as usize;
                    let dst = pixels[idx];

                    let out_a = src_a + (dst.alpha() as u32 * inv_a) / 255;
                    let out_r = src_r + (dst.red()   as u32 * inv_a) / 255;
                    let out_g = src_g + (dst.green() as u32 * inv_a) / 255;
                    let out_b = src_b + (dst.blue()  as u32 * inv_a) / 255;

                    if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(
                        out_r as u8, out_g as u8, out_b as u8, out_a as u8
                    ) {
                        pixels[idx] = c;
                    }
                }
            }
        },
    );
}