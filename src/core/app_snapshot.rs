use super::super::text_buffer::lines::*;

#[derive(Clone)]
pub struct AppSnapshot {
    pub buffer: Lines,
}