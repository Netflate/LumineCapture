use cosmic_text::{Buffer, FontSystem, SwashCache};

pub struct TextState {
    pub buffer: Buffer,   
    pub dirty: bool,      
}