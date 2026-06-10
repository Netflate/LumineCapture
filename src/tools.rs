use strum::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Tool {
    Selection, 
    Rectangle, 
    Arrow, 
    Circle,
    Text,
}
