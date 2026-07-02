use crate::tools::ToolBehavior;
use crate::types::{MouseButton, Annotation, AnnotationShape, TextEditState, SpecialKey};
use crate::editor::EditorState;
use crate::types::annotations::{begin_drag_for_annotation, apply_annotation_drag, commit_drag_if_changed};

use tiny_skia::{Rect, Color};
use std::collections::HashMap;
use cosmic_text::{Buffer, FontSystem, Attrs, Family, Metrics, Shaping};

pub struct TextTool;

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
            if matches!(ann.shape, AnnotationShape::Text { .. }) && ann.initial_hit_test(state.pointer.global) {
                if let Some(prev) = state.text_editing.as_ref() {
                    if prev.annotation_id != ann.id {
                        if let Some(prev_ann) = state.annotations.iter().find(|a| a.id == prev.annotation_id) {
                            state.damage_rects.push(prev_ann.bbox);
                        }
                    }
                }

                state.damage_rects.push(ann.bbox);

                state.text_editing = Some(TextEditState {
                    annotation_id: ann.id,
                    cursor: match &ann.shape {
                        AnnotationShape::Text { content, .. } => content.len(),
                        _ => 0,
                    },
                });
                state.selected_annotation = Some(i);
                state.annotations_dirty = true;
                begin_drag_for_annotation(state, i);
                
                return;
            }
        }

        // 2. clicking on empty > remove prev text focus if exists
        if let Some(edit) = state.text_editing.take() {
            if let Some(prev_ann) = state.annotations.iter().find(|a| a.id == edit.annotation_id) {
                state.damage_rects.push(prev_ann.bbox);
            }
            state.selected_annotation = None;
            state.annotations_dirty = true;
            return;
        }

        // 3. creating new text field
        let id = state.next_id;
        state.next_id += 1;

        let mut ann = Annotation {
            id,
            shape: AnnotationShape::Text {
                start: pos,
                content: String::new(),
                font_size: 24.0,
            },
            color: Color::from_rgba8(255, 255, 255, 255),
            stroke_width: 0.0,
            bbox: Rect::from_xywh(pos.0, pos.1, 1.0, 1.0).unwrap(),
        };
        update_text_bbox(&mut ann, &mut state.font_system, &mut state.text_buffers);
        
        state.push_undo();
        
        state.damage_rects.push(ann.bbox);
        state.annotations.push(ann);
        
        state.text_editing = Some(TextEditState {
            annotation_id: id,
            cursor: 0,
        });
        state.selected_annotation = Some(state.annotations.len() - 1);
        state.annotations_dirty = true;
        
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _sel_dirty: &mut bool, _dirty_mask: &mut u32) {
        apply_annotation_drag(state, global);
    }

    fn on_text(&self, state: &mut EditorState, ch: char, _dirty_mask: &mut u32) {
        let Some(edit) = state.text_editing.as_mut() else { return };
        let id = edit.annotation_id;
        
        let Some(ann) = state.annotations.iter_mut().find(|a| a.id == id) else { return };
        let AnnotationShape::Text { content, .. } = &mut ann.shape else { return };

        state.damage_rects.push(ann.bbox);

        // guard against stale cursor
        let cursor = clamp_to_char_boundary(content, edit.cursor);
        edit.cursor = cursor;

        content.insert(edit.cursor, ch);
        edit.cursor += ch.len_utf8();

        update_text_bbox(ann, &mut state.font_system, &mut state.text_buffers);

        state.damage_rects.push(ann.bbox); 

        state.annotations_dirty = true;
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
        let (id, mut cursor) = {
            let Some(edit) = &state.text_editing else { return };
            (edit.annotation_id, edit.cursor)
        };

        // to avoid bbox recalc, we do that only if needed
        let mut text_changed = false;

        if let Some(ann) = state.annotations.iter_mut().find(|a| a.id == id) {
            let AnnotationShape::Text { content, .. } = &mut ann.shape else { return };

            // guard against stale cursor
            cursor = clamp_to_char_boundary(content, cursor);

            match key {
                SpecialKey::Backspace => {
                    if cursor > 0 {
                        state.damage_rects.push(ann.bbox);
                        cursor -= 1;
                        while !content.is_char_boundary(cursor) { cursor -= 1; }
                        content.remove(cursor);
                        text_changed = true;
                    }
                }
                SpecialKey::Left => {
                    if cursor > 0 {
                        cursor -= 1;
                        while !content.is_char_boundary(cursor) { cursor -= 1; }
                    }
                }
                SpecialKey::Right => {
                    if cursor < content.len() {
                        cursor += 1;
                        while cursor < content.len() && !content.is_char_boundary(cursor) { cursor += 1; }
                    }
                }
                SpecialKey::Enter => {
                }
                _ => {}
            }
        }

        if matches!(key, SpecialKey::Enter) {
            self.on_text(state, '\n', dirty_mask);
            return;
        }

        if let Some(edit) = &mut state.text_editing {
            edit.cursor = cursor;
        }

        if text_changed {
            if let Some(ann) = state.annotations.iter_mut().find(|a| a.id == id) {
                update_text_bbox(ann, &mut state.font_system, &mut state.text_buffers);
                state.damage_rects.push(ann.bbox); 
                state.annotations_dirty = true;
            }
        }
    }

    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        if let Some(edit) = state.text_editing.take() {
            if let Some(ann) = state.annotations.iter().find(|a| a.id == edit.annotation_id) {
                state.damage_rects.push(ann.bbox);
            }
        }
        state.text_editing = None;
        state.selected_annotation = None;
        state.annotations_dirty = true;
    }
}

fn clamp_to_char_boundary(s: &str, cursor: usize) -> usize {
    let clamped = cursor.min(s.len());
    (0..=clamped).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0)
}

// separate updating bbox function from annotation.rs
// since we need some editor state parameters
pub fn update_text_bbox(
    ann: &mut Annotation, 
    font_system: &mut FontSystem, 
    text_buffers: &mut HashMap<u64, Buffer>
) {
    let AnnotationShape::Text { start, content, font_size } = &ann.shape else { return };
    let (x, y) = *start;
    let font_size = *font_size;
    let id = ann.id;

    let metrics = Metrics::new(font_size, font_size * 1.2);

    let buffer = text_buffers.entry(id).or_insert_with(|| {
        Buffer::new_empty(metrics) 
    });

    if buffer.metrics() != metrics {
        buffer.set_metrics(metrics);
    }
    
    buffer.set_size(None, None);
    
    buffer.set_text(
        content, 
        &Attrs::new().family(Family::SansSerif), 
        Shaping::Advanced, 
        None 
    );
    
    buffer.shape_until_scroll(font_system, false);

    let mut max_w = 0.0_f32;
    let mut total_h = 0.0_f32;

    for run in buffer.layout_runs() {
        if run.line_w > max_w {
            max_w = run.line_w;
        }
        total_h = run.line_y + metrics.line_height;
    }

    let w = max_w.max(10.0);
    let h = if total_h > 0.0 { total_h } else { metrics.line_height };

    ann.bbox = Rect::from_xywh(x, y, w, h).unwrap_or_else(|| {
        Rect::from_xywh(x, y, 10.0, metrics.line_height).unwrap()
    });
}