use crate::editor::EditorState;

impl EditorState {
    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.annotations.clone());
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, dirty_mask: &mut u32) {
        // no point in keeping in redo what wasn't commited
        if self.pending.is_some() {
            if let Some(ann) = &self.pending {
                self.damage_rects.push(ann.bbox); 
            }
            self.pending = None;
            self.prev_pending = None;
            *dirty_mask = u32::MAX;
            return;
        }
        if let Some(prev_state) = self.undo_stack.pop() {
            Self::record_history_damage(&mut self.damage_rects, &self.annotations, &prev_state);
            self.redo_stack.push(self.annotations.clone());
            self.annotations = prev_state;
            self.selected_annotation = None;  
            self.ann_drag = None;    
            self.annotations_dirty = true;
         
            *dirty_mask = u32::MAX;
        }
    }
    
    pub fn redo(&mut self, dirty_mask: &mut u32) {
        if let Some(next_state) = self.redo_stack.pop() {
            Self::record_history_damage(&mut self.damage_rects, &self.annotations, &next_state);
            
            self.annotations_dirty = true;
            self.undo_stack.push(self.annotations.clone());
            self.annotations = next_state;
            self.selected_annotation = None; 
            self.ann_drag = None;             
            *dirty_mask = u32::MAX;
        }
    }
}