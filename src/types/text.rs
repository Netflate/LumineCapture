use cosmic_text::Buffer;

pub struct TextState {
    pub buffer: Buffer,
    pub dirty: bool,
}

pub struct TextEditState {
    pub annotation_id: u64,
    pub cursor: usize,
}
