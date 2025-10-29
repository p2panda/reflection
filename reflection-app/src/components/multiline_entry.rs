use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct MultilineEntry {}

    #[glib::object_subclass]
    impl ObjectSubclass for MultilineEntry {
        const NAME: &'static str = "MultilineEntry";
        type Type = super::MultilineEntry;
        type ParentType = gtk::TextView;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("entry");
        }
    }

    impl ObjectImpl for MultilineEntry {
        fn constructed(&self) {
            let key_events = gtk::EventControllerKey::new();
            key_events.connect_key_pressed(|controller, key, _, modifier| {
                if modifier.is_empty() && (key == gdk::Key::Return || key == gdk::Key::KP_Enter) {
                    if let Some(widget) = controller.widget() {
                        widget.activate_default();
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.obj().add_controller(key_events);
        }
    }
    impl WidgetImpl for MultilineEntry {}
    impl TextViewImpl for MultilineEntry {}
}

glib::wrapper! {
    pub struct MultilineEntry(ObjectSubclass<imp::MultilineEntry>)
        @extends gtk::Widget, gtk::TextView,
         @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl MultilineEntry {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
