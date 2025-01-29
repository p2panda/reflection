mod crdt;

use crate::crdt::TextCrdt;

pub struct Document {
    #[allow(dead_code)]
    text_crdt: TextCrdt,
}
