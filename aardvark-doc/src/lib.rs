pub mod author;
pub mod authors;
pub mod document;
pub mod service;

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::service::Service;
    use glib::object::ObjectExt;

    #[test]
    fn create_document() {
        let context = glib::MainContext::default();
        let service = Service::new();
        let test_string = "Hello World";
        service.startup();
        let document = Document::new(&service, None);
        context.iteration(false);
        assert!(document.insert_text(0, test_string).is_ok());
        assert_eq!(document.text(), test_string);
    }

    #[test]
    fn basic_sync() {
        let main_loop = glib::MainLoop::new(None, false);
        let test_string = "Hello World";
        let service = Service::new();
        service.startup();

        let document = Document::new(&service, None);
        let id = document.id();

        let service2 = Service::new();
        service2.startup();
        let document2 = Document::new(&service2, Some(&id));

        assert_eq!(document.id(), document2.id());
        main_loop.context().spawn(async move {
            assert!(document.insert_text(0, test_string).is_ok());
            assert_eq!(document.text(), test_string);
        });

        let main_loop_clone = main_loop.clone();
        document2.connect_notify(Some("text"), move |_, _| {
            main_loop_clone.quit();
        });

        main_loop.run();
        service.shutdown();
        service2.shutdown();

        assert_eq!(document2.text(), test_string);
    }

    #[test]
    fn sync_multiple_changes() {
        let main_loop = glib::MainLoop::new(None, false);
        let expected_string = "Hello, World!";
        let service = Service::new();
        service.startup();

        let document = Document::new(&service, None);
        let id = document.id();

        let service2 = Service::new();
        service2.startup();
        let document2 = Document::new(&service2, Some(&id));

        assert_eq!(document.id(), document2.id());
        main_loop.context().spawn(async move {
            assert!(document.insert_text(0, "Hello,").is_ok());
            assert!(document.insert_text(6, " World!").is_ok());
            assert!(document.delete_range(7, 8).is_ok());
            assert!(document.insert_text(7, "W").is_ok());
            assert_eq!(document.text(), expected_string);
        });

        let main_loop_clone = main_loop.clone();
        document2.connect_notify(Some("text"), move |document2, _| {
            if document2.text() == expected_string {
                main_loop_clone.quit();
            }
        });

        main_loop.run();

        service.shutdown();
        service2.shutdown();
    }

    #[test]
    fn sync_longer_text() {
        let main_loop = glib::MainLoop::new(None, false);
        let test_string = "Et aut omnis eos corporis ut. Qui est blanditiis blanditiis.
        Sit quia nam maxime accusantium ut voluptatem. Fuga consequuntur animi et et est.
        Unde voluptas consequatur mollitia id odit optio harum sint. Fugit quo aut et laborum aut cupiditate.";
        let expected_string = format!(
            "{}{}{}{}",
            test_string, test_string, test_string, test_string
        );
        let service = Service::new();
        service.startup();

        let document = Document::new(&service, None);
        let id = document.id();

        let service2 = Service::new();
        service2.startup();
        let document2 = Document::new(&service2, Some(&id));

        assert_eq!(document.id(), document2.id());
        main_loop.context().spawn(async move {
            assert!(document.insert_text(0, test_string).is_ok());
            assert!(document.insert_text(0, test_string).is_ok());
            assert!(document.insert_text(0, test_string).is_ok());
            assert!(document.insert_text(0, test_string).is_ok());
        });

        let main_loop_clone = main_loop.clone();
        document2.connect_notify(Some("text"), move |document2, _| {
            if document2.text() == expected_string {
                main_loop_clone.quit();
            }
        });

        main_loop.run();

        service.shutdown();
        service2.shutdown();
    }
}
