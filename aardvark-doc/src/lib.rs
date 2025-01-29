mod crdt;

pub use crate::crdt::{TextCrdt, TextCrdtError, TextCrdtEvent, TextDelta};

pub struct Document {
    #[allow(dead_code)]
    text_crdt: TextCrdt,
}
