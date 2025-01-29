mod crdt;

pub use crate::crdt::{TextCrdt, TextCrdtError, TextCrdtEvent};

pub struct Document {
    #[allow(dead_code)]
    text_crdt: TextCrdt,
}
