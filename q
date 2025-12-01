[33mcommit 806460d42d656344bbb88ea46ac980b0147f4189[m[33m ([m[1;36mHEAD[m[33m -> [m[1;32mjsparber/new_document_flow[m[33m)[m
Author: Julian Sparber <julian@sparber.net>
Date:   Mon Dec 1 16:00:24 2025 +0100

    app: Add app actions for creating, joining, deleting documents

[1mdiff --git a/reflection-app/src/application.rs b/reflection-app/src/application.rs[m
[1mindex 99b894f..99cf34c 100644[m
[1m--- a/reflection-app/src/application.rs[m
[1m+++ b/reflection-app/src/application.rs[m
[36m@@ -28,6 +28,7 @@[m [muse thiserror::Error;[m
 use tracing::error;[m
 [m
 use crate::config;[m
[32m+[m[32muse crate::open_dialog::OpenDialog;[m
 use crate::secret;[m
 use crate::system_settings::SystemSettings;[m
 use crate::window::Window;[m
[36m@@ -157,6 +158,48 @@[m [mimpl ReflectionApplication {[m
         let new_window_action = gio::ActionEntry::builder("new-window")[m
             .activate(move |app: &Self, _, _| app.new_window())[m
             .build();[m
[32m+[m[32m        let new_document_action = gio::ActionEntry::builder("new-document")[m
[32m+[m[32m            .activate(move |app: &Self, _, _| app.new_document())[m
[32m+[m[32m            .build();[m
[32m+[m[32m        let join_document_action = gio::ActionEntry::builder("join-document")[m
[32m+[m[32m            .parameter_type(Some(&glib::VariantType::new_array([m
[32m+[m[32m                &DocumentId::static_variant_type(),[m
[32m+[m[32m            )))[m
[32m+[m[32m            .activate(move |app: &Self, _, parameter| {[m
[32m+[m[32m                let parameter = parameter.unwrap();[m
[32m+[m
[32m+[m[32m                if parameter.n_children() == 0 {[m
[32m+[m[32m                    app.open_join_document_dialog();[m
[32m+[m[32m                } else {[m
[32m+[m[32m                    for i in 0..parameter.n_children() {[m
[32m+[m[32m                        if let Some(document_id) = parameter.child_value(i).get() {[m
[32m+[m[32m                            app.join_document(&document_id);[m
[32m+[m[32m                            // FIXME: open all documents with it's own window[m
[32m+[m[32m                            break;[m
[32m+[m[32m                        } else {[m
[32m+[m[32m                            error!("Failed to join document: Invalid document id specified");[m
[32m+[m[32m                        }[m
[32m+[m[32m                    }[m
[32m+[m[32m                }[m
[32m+[m[32m            })[m
[32m+[m[32m            .build();[m
[32m+[m[32m        let delete_document_action = gio::ActionEntry::builder("delete-document")[m
[32m+[m[32m            .parameter_type(Some(&glib::VariantType::new_array([m
[32m+[m[32m                &DocumentId::static_variant_type(),[m
[32m+[m[32m            )))[m
[32m+[m[32m            .activate(move |app: &Self, _, parameter| {[m
[32m+[m[32m                let parameter = parameter.unwrap();[m
[32m+[m
[32m+[m[32m                for i in 0..parameter.n_children() {[m
[32m+[m[32m                    if let Some(document_id) = parameter.child_value(i).get() {[m
[32m+[m[32m                        app.delete_document(&document_id);[m
[32m+[m[32m                        break;[m
[32m+[m[32m                    } else {[m
[32m+[m[32m                        error!("Failed to delete document: Invalid document id specified");[m
[32m+[m[32m                    }[m
[32m+[m[32m                }[m
[32m+[m[32m            })[m
[32m+[m[32m            .build();[m
         let temporary_identity_action = gio::ActionEntry::builder("new-temporary-identity")[m
             .activate(move |app: &Self, _, _| {[m
                 glib::spawn_future_local(clone!([m
[36m@@ -168,10 +211,14 @@[m [mimpl ReflectionApplication {[m
                 ));[m
             })[m
             .build();[m
[32m+[m
         self.add_action_entries([[m
             quit_action,[m
             about_action,[m
             new_window_action,[m
[32m+[m[32m            new_document_action,[m
[32m+[m[32m            join_document_action,[m
[32m+[m[32m            delete_document_action,[m
             temporary_identity_action,[m
         ]);[m
     }[m
[36m@@ -185,6 +232,57 @@[m [mimpl ReflectionApplication {[m
         window.present();[m
     }[m
 [m
[32m+[m[32m    fn new_document(&self) {[m
[32m+[m[32m        self.join_document(&DocumentId::new());[m
[32m+[m[32m    }[m
[32m+[m
[32m+[m[32m    fn open_join_document_dialog(&self) {[m
[32m+[m[32m        let active = self.active_window();[m
[32m+[m
[32m+[m[32m        let dialog = OpenDialog::new();[m
[32m+[m[32m        adw::prelude::AdwDialogExt::present(&dialog, active.as_ref());[m
[32m+[m[32m    }[m
[32m+[m
[32m+[m[32m    fn join_document(&self, document_id: &DocumentId) {[m
[32m+[m[32m        if let Some(window) = self.window_for_document_id(document_id) {[m
[32m+[m[32m            window.present();[m
[32m+[m[32m        } else {[m
[32m+[m[32m            let Some(service) = self.service() else {[m
[32m+[m[32m                return;[m
[32m+[m[32m            };[m
[32m+[m[32m            let Some(active) = self.active_window().and_downcast::<Window>() else {[m
[32m+[m[32m                return;[m
[32m+[m[32m            };[m
[32m+[m[32m            let document = service.join_document(document_id);[m
[32m+[m[32m            active.set_document(Some(&document));[m
[32m+[m[32m            let hold_guard = self.hold();[m
[32m+[m[32m            glib::spawn_future_local(clone!([m
[32m+[m[32m                #[weak][m
[32m+[m[32m                document,[m
[32m+[m[32m                async move {[m
[32m+[m[32m                    document.subscribe().await;[m
[32m+[m[32m                    drop(hold_guard);[m
[32m+[m[32m                }[m
[32m+[m[32m            ));[m
[32m+[m[32m        }[m
[32m+[m[32m    }[m
[32m+[m
[32m+[m[32m    fn delete_document(&self, document_id: &DocumentId) {[m
[32m+[m[32m        if let Some(service) = self.service()[m
[32m+[m[32m            && let Some(document) = service.documents().document(document_id)[m
[32m+[m[32m        {[m
[32m+[m[32m            let hold_guard = self.hold();[m
[32m+[m[32m            glib::spawn_future_local(clone!([m
[32m+[m[32m                #[strong][m
[32m+[m[32m                document,[m
[32m+[m[32m                async move {[m
[32m+[m[32m                    document.delete().await;[m
[32m+[m[32m                    drop(hold_guard);[m
[32m+[m[32m                }[m
[32m+[m[32m            ));[m
[32m+[m[32m        }[m
[32m+[m[32m    }[m
[32m+[m
     async fn new_temporary_identity(&self) {[m
         let private_key = PrivateKey::new();[m
         let service = Service::new(&private_key, None);[m
[36m@@ -209,26 +307,13 @@[m [mimpl ReflectionApplication {[m
             return;[m
         }[m
 [m
[31m-        self.imp().service.replace(Some(service));[m
[31m-[m
[31m-        // FIXME: We can't use block_on() inside an async context[m
[31m-        // New documents block on creating the document id, probably[m
[31m-        // we should make document creating async[m
[31m-        glib::source::idle_add_local(clone!([m
[31m-            #[weak(rename_to = this)][m
[31m-            self,[m
[31m-            #[upgrade_or][m
[31m-            glib::ControlFlow::Break,[m
[31m-            move || {[m
[31m-                let service = this.service();[m
[31m-                for window in this.windows() {[m
[31m-                    if let Ok(window) = window.downcast::<Window>() {[m
[31m-                        window.set_service(service.as_ref());[m
[31m-                    }[m
[31m-                }[m
[31m-                glib::ControlFlow::Break[m
[32m+[m[32m        for window in self.windows() {[m
[32m+[m[32m            if let Ok(window) = window.downcast::<Window>() {[m
[32m+[m[32m                window.set_service(Some(&service));[m
             }[m
[31m-        ));[m
[32m+[m[32m        }[m
[32m+[m
[32m+[m[32m        self.imp().service.replace(Some(service));[m
     }[m
 [m
     fn show_about(&self) {[m
[1mdiff --git a/reflection-app/src/document_view.rs b/reflection-app/src/document_view.rs[m
[1mindex 6ccd700..94faee6 100644[m
[1m--- a/reflection-app/src/document_view.rs[m
[1m+++ b/reflection-app/src/document_view.rs[m
[36m@@ -63,7 +63,7 @@[m [mmod imp {[m
         pub zoom_level: Cell<f64>,[m
         #[property(get, construct_only)][m
         pub service: OnceCell<Service>,[m
[31m-        #[property(get, type = Document)][m
[32m+[m[32m        #[property(get, set = Self::set_document, nullable)][m
         document: RefCell<Option<Document>>,[m
     }[m
 [m
[36m@@ -210,7 +210,7 @@[m [mmod imp {[m
                     if let Some(window) = app.window_for_document_id(&document.id()) {[m
                         window.present();[m
                     } else {[m
[31m-                        this.set_document(document.to_owned());[m
[32m+[m[32m                        this.set_document(Some(document.to_owned()));[m
                         let hold_guard = ReflectionApplication::default().hold();[m
                         glib::spawn_future_local(clone!([m
                             #[weak][m
[36m@@ -228,14 +228,17 @@[m [mmod imp {[m
                 #[weak(rename_to = this)][m
                 self,[m
                 move |button| {[m
[31m-                    let document_id = Self::format_document_id(&this.obj().document().id());[m
[32m+[m[32m                    let Some(document) = this.obj().document() else {[m
[32m+[m[32m                        return;[m
[32m+[m[32m                    };[m
[32m+[m[32m                    let document_id = Self::format_document_id(&document.id());[m
                     let clipboard = button.display().clipboard();[m
                     clipboard.set(&document_id);[m
                     this.share_popover.popdown();[m
                 }[m
             ));[m
 [m
[31m-            self.set_document(self.obj().service().join_document(&DocumentId::new()));[m
[32m+[m[32m            self.set_document(Some(self.obj().service().join_document(&DocumentId::new())));[m
         }[m
     }[m
 [m
[36m@@ -253,29 +256,28 @@[m [mmod imp {[m
             self.obj().action_set_enabled("window.zoom-out", size > 1.0);[m
         }[m
 [m
[31m-        fn set_document(&self, document: Document) {[m
[31m-            let document_id = Self::format_document_id(&document.id());[m
[31m-            self.share_code_label.set_text(&document_id);[m
[32m+[m[32m        fn set_document(&self, document: Option<Document>) {[m
[32m+[m[32m            if let Some(ref document) = document {[m
[32m+[m[32m                let document_id = Self::format_document_id(&document.id());[m
[32m+[m[32m                self.share_code_label.set_text(&document_id);[m
[32m+[m[32m            }[m
[32m+[m
             self.text_view[m
                 .buffer()[m
                 .downcast::<ReflectionTextBuffer>()[m
                 .unwrap()[m
[31m-                .set_document(&document);[m
[32m+[m[32m                .set_document(document.as_ref());[m
 [m
[31m-            let old_document = self.document.replace(Some(document));[m
[32m+[m[32m            let old_document = self.document.replace(document);[m
 [m
             if let Some(old_document) = old_document {[m
                 // We need to make sure that unsubscribe runs[m
                 // to termination before the app is terminated[m
                 let hold_guard = ReflectionApplication::default().hold();[m
[31m-                glib::spawn_future_local(clone!([m
[31m-                    #[weak][m
[31m-                    old_document,[m
[31m-                    async move {[m
[31m-                        old_document.unsubscribe().await;[m
[31m-                        drop(hold_guard);[m
[31m-                    }[m
[31m-                ));[m
[32m+[m[32m                glib::spawn_future_local(async move {[m
[32m+[m[32m                    old_document.unsubscribe().await;[m
[32m+[m[32m                    drop(hold_guard);[m
[32m+[m[32m                });[m
             }[m
 [m
             self.obj().notify("document");[m
[1mdiff --git a/reflection-app/src/error_page.blp b/reflection-app/src/error_page.blp[m
[1mindex 4b1906f..0ebd71d 100644[m
[1m--- a/reflection-app/src/error_page.blp[m
[1m+++ b/reflection-app/src/error_page.blp[m
[36m@@ -1,10 +1,12 @@[m
 using Gtk 4.0;[m
 using Adw 1;[m
 [m
[31m-template $ReflectionErrorPage: Adw.Bin {[m
[32m+[m[32mtemplate $ReflectionErrorPage: Adw.NavigationPage {[m
   Adw.ToolbarView {[m
     [top][m
     Adw.HeaderBar {[m
[32m+[m[32m      show-back-button: false;[m
[32m+[m
       title-widget: Adw.WindowTitle {[m
         title: _("Error");[m
       };[m
[1mdiff --git a/reflection-app/src/textbuffer.rs b/reflection-app/src/textbuffer.rs[m
[1mindex 4317343..54f179d 100644[m
[1m--- a/reflection-app/src/textbuffer.rs[m
[1m+++ b/reflection-app/src/textbuffer.rs[m
[36m@@ -43,7 +43,7 @@[m [mmod imp {[m
         pub inhibit_text_change: Cell<bool>,[m
         pub document_handlers: OnceCell<glib::SignalGroup>,[m
         pub changed_handler: RefCell<Option<glib::SignalHandlerId>>,[m
[31m-        #[property(get, set = Self::set_document)][m
[32m+[m[32m        #[property(get, set = Self::set_document, nullable)][m
         pub document: RefCell<Option<Document>>,[m
         #[property(name = "custom-can-undo", get = Self::custom_can_undo, type = bool)][m
         #[property(name = "custom-can-redo", get = Self::custom_can_redo, type = bool)][m
[1mdiff --git a/reflection-app/src/window.rs b/reflection-app/src/window.rs[m
[1mindex 9700c8a..6272edc 100644[m
[1m--- a/reflection-app/src/window.rs[m
[1m+++ b/reflection-app/src/window.rs[m
[36m@@ -41,7 +41,7 @@[m [mmod imp {[m
         pub(super) error_page: TemplateChild<ErrorPage>,[m
         #[property(get = Self::service, set = Self::set_service, nullable)][m
         service: PhantomData<Option<Service>>,[m
[31m-        #[property(get = Self::document, nullable)][m
[32m+[m[32m        #[property(get = Self::document, set = Self::set_document, nullable)][m
         document: PhantomData<Option<Document>>,[m
     }[m
 [m
[36m@@ -94,7 +94,13 @@[m [mmod imp {[m
         }[m
 [m
         fn document(&self) -> Option<Document> {[m
[31m-            Some(self.document_view()?.document())[m
[32m+[m[32m            self.document_view()?.document()[m
[32m+[m[32m        }[m
[32m+[m
[32m+[m[32m        fn set_document(&self, document: Option<Document>) {[m
[32m+[m[32m            if let Some(document_view) = self.document_view() {[m
[32m+[m[32m                document_view.set_document(document);[m
[32m+[m[32m            }[m
         }[m
     }[m
 [m
