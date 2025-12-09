use adw::{prelude::*, subclass::prelude::*};
use gtk::{glib, gsk};

mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::marker::PhantomData;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::IndicatorBin)]
    pub struct IndicatorBin {
        #[property(get = Self::show_indicator, set = Self::set_show_indicator)]
        show_indicator: PhantomData<bool>,
        #[property(get = Self::indicator, set = Self::set_indicator, nullable, type = Option<gtk::Widget>)]
        indicator: adw::Bin,
        #[property(get = Self::child, set = Self::set_child, nullable, type = Option<gtk::Widget>)]
        child: RefCell<Option<gtk::Widget>>,
        mask: adw::Bin,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for IndicatorBin {
        const NAME: &'static str = "ReflectionIndicatorBin";
        type Type = super::IndicatorBin;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("indicatorbin");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for IndicatorBin {
        fn constructed(&self) {
            self.parent_constructed();
            self.indicator.set_parent(&*self.obj());
            self.indicator.set_halign(gtk::Align::End);
            self.indicator.set_valign(gtk::Align::End);
            self.indicator.set_can_target(false);
            self.indicator.add_css_class("indicator");
            self.mask.set_parent(&*self.obj());
            self.mask.set_halign(gtk::Align::End);
            self.mask.set_valign(gtk::Align::End);
            self.mask.set_can_target(false);
            self.mask.add_css_class("mask");
        }

        fn dispose(&self) {
            self.indicator.unparent();
            self.mask.unparent();
            self.set_child(None);
        }
    }

    impl IndicatorBin {
        fn indicator(&self) -> Option<gtk::Widget> {
            self.indicator.child()
        }

        fn set_indicator(&self, widget: Option<&gtk::Widget>) {
            self.indicator.set_child(widget);
        }

        fn child(&self) -> Option<gtk::Widget> {
            self.child.borrow().to_owned()
        }

        fn set_child(&self, widget: Option<gtk::Widget>) {
            let mut borrow = self.child.borrow_mut();

            if let Some(child) = borrow.take() {
                child.unparent();
            }

            if let Some(child) = widget {
                child.set_parent(&*self.obj());
                borrow.replace(child);
            }
        }

        fn show_indicator(&self) -> bool {
            self.mask.is_visible() && self.indicator.is_visible()
        }

        fn set_show_indicator(&self, visible: bool) {
            self.mask.set_visible(visible);
            self.indicator.set_visible(visible);
        }
    }

    impl WidgetImpl for IndicatorBin {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let borrow = self.child.borrow();
            let Some(child) = borrow.as_ref() else {
                return;
            };

            snapshot.push_mask(gsk::MaskMode::InvertedAlpha);
            self.obj().snapshot_child(&self.mask, snapshot);
            snapshot.pop();

            self.obj().snapshot_child(child, snapshot);
            snapshot.pop();
            self.obj().snapshot_child(&self.indicator, snapshot);
        }
    }
}

glib::wrapper! {
    pub struct IndicatorBin(ObjectSubclass<imp::IndicatorBin>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl IndicatorBin {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
