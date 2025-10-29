use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, glib::GString};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::Avatar)]
    pub struct Avatar {
        #[property(name = "emoji", get = Self::emoji, set = Self::set_emoji, type = GString)]
        label: gtk::Label,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Avatar {
        const NAME: &'static str = "Avatar";
        type Type = super::Avatar;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for Avatar {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().set_child(Some(&self.label));
            self.obj().add_css_class("avatar");
            self.obj().set_valign(gtk::Align::Center);
            self.obj().set_halign(gtk::Align::Center);
        }
    }

    impl Avatar {
        fn emoji(&self) -> GString {
            self.label.label()
        }

        fn set_emoji(&self, emoji: &str) {
            self.label.set_label(emoji);
        }
    }

    impl WidgetImpl for Avatar {}
    impl BinImpl for Avatar {}
}

glib::wrapper! {
    pub struct Avatar(ObjectSubclass<imp::Avatar>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Avatar {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
