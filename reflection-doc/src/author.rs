use std::sync::Mutex;
use std::{cell::Cell, sync::OnceLock};

use glib::Properties;
use glib::prelude::*;
use glib::subclass::prelude::*;

use crate::identity::PublicKey;

pub const COLORS: [(&str, &str); 14] = [
    ("Yellow", "#faf387"),
    ("Orange", "#ffc885"),
    ("Red", "#f99085"),
    ("Pink", "#fcaed5"),
    ("Purple", "#f39bf2"),
    ("Violet", "#b797f3"),
    ("Blue", "#99c1f1"),
    ("Cyan", "#99f1ec"),
    ("Green", "#97f1aa"),
    ("Brown", "#d9c0ab"),
    ("Silver", "#deddda"),
    ("Gray", "#c0bfbc"),
    ("Black", "#9a9996"),
    ("Gold", "#ead688"),
];

pub const EMOJIS: [(&str, &str); 41] = [
    ("🐵", "Monkey"),
    ("🐶", "Dog"),
    ("🐱", "Cat"),
    ("🦊", "Fox"),
    ("🐺", "Wolf"),
    ("🦝", "Raccoon"),
    ("🦁", "Lion"),
    ("🐯", "Tiger"),
    ("🐷", "Pig"),
    ("🐴", "Horse"),
    ("🦄", "Unicorn"),
    ("🦓", "Zebra"),
    ("🫎", "Moose"),
    ("🐔", "Chicken"),
    ("🐼", "Panda"),
    ("🐻", "Bear"),
    ("🐻‍❄️", "Polar Bear"),
    ("🐨", "Koala"),
    ("🐸", "Frog"),
    ("🐹", "Hamster"),
    ("🐰", "Rabbit"),
    ("🐮", "Cow"),
    ("🐝", "Bee"),
    ("🐢", "Turtle"),
    ("🐏", "Ram"),
    ("🐳", "Whale"),
    ("🐙", "Octopus"),
    ("🦀", "Crab"),
    ("🐌", "Snail"),
    ("🪲", "Beetle"),
    ("🐞", "Ladybug"),
    ("🦈", "Shark"),
    ("🦭", "Seal"),
    ("🐟", "Fish"),
    ("🦆", "Duck"),
    ("🦥", "Sloth"),
    ("🦫", "Beaver"),
    ("🐪", "Camel"),
    ("🦍", "Gorilla"),
    ("🦣", "Mammooth"),
    ("🐃", "Buffalo"),
];

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Author)]
    pub struct Author {
        #[property(name = "name", get = Self::name, type = String)]
        #[property(name = "emoji", get = Self::emoji, type = String)]
        #[property(name = "color", get = Self::color, type = String)]
        #[property(name = "hex-color", get = Self::hex_color, type = String)]
        #[property(get, set, construct_only, type = PublicKey)]
        public_key: OnceLock<PublicKey>,
        #[property(get, set, construct_only)]
        pub last_seen: Mutex<Option<glib::DateTime>>,
        #[property(get, default = true)]
        pub is_online: Cell<bool>,
        #[property(get)]
        pub is_this_device: Cell<bool>,
        pub last_cursor_update: Mutex<Option<std::time::SystemTime>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Author {
        const NAME: &'static str = "Author";
        type Type = super::Author;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Author {}

    impl Author {
        fn name(&self) -> String {
            let bytes = self.public_key.get().unwrap().as_bytes();
            let selector_color = bytes[..(bytes.len() / 2)]
                .iter()
                .fold(0u8, |acc, b| acc ^ b) as usize
                % COLORS.len();
            let selector_emoji = bytes[(bytes.len() / 2)..]
                .iter()
                .fold(0u8, |acc, b| acc ^ b) as usize
                % EMOJIS.len();
            format!("{} {}", COLORS[selector_color].0, EMOJIS[selector_emoji].1)
        }

        fn emoji(&self) -> String {
            let bytes = self.public_key.get().unwrap().as_bytes();
            let selector_emoji = bytes[(bytes.len() / 2)..]
                .iter()
                .fold(0u8, |acc, b| acc ^ b) as usize
                % EMOJIS.len();
            EMOJIS[selector_emoji].0.to_string()
        }

        fn color(&self) -> String {
            let bytes = self.public_key.get().unwrap().as_bytes();
            let selector_color = bytes[..(bytes.len() / 2)]
                .iter()
                .fold(0u8, |acc, b| acc ^ b) as usize
                % COLORS.len();
            COLORS[selector_color].0.to_string()
        }

        fn hex_color(&self) -> String {
            let bytes = self.public_key.get().unwrap().as_bytes();
            let selector_color = bytes[..(bytes.len() / 2)]
                .iter()
                .fold(0u8, |acc, b| acc ^ b) as usize
                % COLORS.len();
            COLORS[selector_color].1.to_string()
        }
    }
}

glib::wrapper! {
    pub struct Author(ObjectSubclass<imp::Author>);
}
impl Author {
    pub(crate) fn new(public_key: &PublicKey) -> Self {
        glib::Object::builder()
            .property("public-key", public_key)
            .build()
    }

    pub(crate) fn with_state(public_key: &PublicKey, last_seen: Option<&glib::DateTime>) -> Self {
        glib::Object::builder()
            .property("public-key", public_key)
            .property("last-seen", last_seen)
            .build()
    }

    pub(crate) fn for_this_device(
        public_key: &PublicKey,
        last_seen: Option<&glib::DateTime>,
    ) -> Self {
        let obj = Self::with_state(public_key, last_seen);

        obj.imp().is_this_device.set(true);
        obj
    }

    pub(crate) fn set_online(&self, is_online: bool) {
        let was_online = self.imp().is_online.get();
        self.imp().is_online.set(is_online);
        if !is_online && was_online {
            *self.imp().last_seen.lock().unwrap() = glib::DateTime::now_local().ok();
            self.notify_last_seen();
        }
        self.notify_is_online();
    }

    pub(crate) fn is_new_cursor_position(&self, timestamp: std::time::SystemTime) -> bool {
        let mut last_cursor_update = self.imp().last_cursor_update.lock().unwrap();

        if last_cursor_update.is_none() || timestamp >= last_cursor_update.unwrap() {
            *last_cursor_update = Some(timestamp);
            true
        } else {
            false
        }
    }
}
