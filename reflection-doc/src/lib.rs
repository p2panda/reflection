pub mod author;
pub mod authors;
pub mod document;
pub mod documents;
pub mod service;

pub mod identity {
    use reflection_node::p2panda_core;
    pub use reflection_node::p2panda_core::identity::IdentityError;
    use std::fmt;

    #[derive(Clone, Debug, glib::Boxed)]
    #[boxed_type(name = "ReflectionPrivateKey", nullable)]
    pub struct PrivateKey(pub(crate) p2panda_core::PrivateKey);

    impl PrivateKey {
        pub fn new() -> PrivateKey {
            PrivateKey(p2panda_core::PrivateKey::new())
        }

        pub fn public_key(&self) -> PublicKey {
            PublicKey(self.0.public_key())
        }

        pub fn as_bytes(&self) -> &[u8] {
            self.0.as_bytes().as_slice()
        }
    }

    impl TryFrom<&[u8]> for PrivateKey {
        type Error = p2panda_core::IdentityError;

        fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
            Ok(PrivateKey(p2panda_core::PrivateKey::try_from(value)?))
        }
    }

    impl<'a> From<&'a PrivateKey> for &'a [u8] {
        fn from(value: &PrivateKey) -> &[u8] {
            value.0.as_bytes().as_slice()
        }
    }

    impl fmt::Display for PrivateKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, f)
        }
    }

    #[derive(Clone, Debug, PartialEq, glib::Boxed)]
    #[boxed_type(name = "ReflectionPublicKey", nullable)]
    pub struct PublicKey(pub(crate) p2panda_core::PublicKey);

    impl fmt::Display for PublicKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, f)
        }
    }

    impl<'a> From<&'a PublicKey> for &'a [u8] {
        fn from(value: &PublicKey) -> &[u8] {
            value.0.as_bytes().as_slice()
        }
    }

    impl PublicKey {
        pub fn as_bytes(&self) -> &[u8] {
            self.0.as_bytes().as_slice()
        }
    }
}

#[cfg(test)]
mod tests {
    use glib::clone;
    use test_log::test;

    use crate::document::Document;
    use crate::identity::PrivateKey;
    use crate::service::Service;

    #[test]
    fn create_document() {
        let test_string = "Hello World";

        let context = glib::MainContext::new();
        let main_loop = glib::MainLoop::new(Some(&context), false);

        context.spawn_local(clone!(
            #[strong]
            context,
            #[strong]
            main_loop,
            async move {
                let private_key = PrivateKey::new();
                let service = Service::new(&private_key, None);
                service.startup().await.unwrap();

                let document = Document::create_with_main_context(&service, &context).await;
                document.subscribe().await;

                assert!(document.insert_text(0, test_string).is_ok());
                assert_eq!(document.text(), test_string);

                service.shutdown().await;
                main_loop.quit();
            }
        ));

        main_loop.run();
    }

    #[test]
    fn basic_sync() {
        let test_string = "Hello World";

        let context = glib::MainContext::new();
        let main_loop = glib::MainLoop::new(Some(&context), false);

        context.spawn_local(clone!(
            #[strong]
            context,
            #[strong]
            main_loop,
            async move {
                let private_key = PrivateKey::new();
                let service = Service::new(&private_key, None);
                service.startup().await.unwrap();

                let document = Document::create_with_main_context(&service, &context).await;
                document.subscribe().await;
                let id = document.id();

                let private_key2 = PrivateKey::new();
                let service2 = Service::new(&private_key2, None);
                service2.startup().await.unwrap();

                let document2 = Document::with_main_context(&service2, &id, &context);
                document2.subscribe().await;

                assert_eq!(document.id(), document2.id());

                assert!(document.insert_text(0, test_string).is_ok());
                assert_eq!(document.text(), test_string);

                service.shutdown().await;
                service2.shutdown().await;

                assert_eq!(document2.text(), test_string);

                main_loop.quit();
            }
        ));

        main_loop.run();
    }

    #[test]
    fn sync_multiple_changes() {
        let expected_string = "Hello, World!";

        let context = glib::MainContext::new();
        let main_loop = glib::MainLoop::new(Some(&context), false);

        context.spawn_local(clone!(
            #[strong]
            context,
            #[strong]
            main_loop,
            async move {
                let private_key = PrivateKey::new();
                let service = Service::new(&private_key, None);
                service.startup().await.unwrap();

                let document = Document::create_with_main_context(&service, &context).await;
                document.subscribe().await;
                let id = document.id();

                let private_key2 = PrivateKey::new();
                let service2 = Service::new(&private_key2, None);
                service2.startup().await.unwrap();

                let document2 = Document::with_main_context(&service2, &id, &context);
                document2.subscribe().await;

                assert_eq!(document.id(), document2.id());

                assert!(document.insert_text(0, "Hello,").is_ok());
                assert!(document.insert_text(6, " World!").is_ok());
                assert!(document.delete_range(7, 8).is_ok());
                assert!(document.insert_text(7, "W").is_ok());
                assert_eq!(document.text(), expected_string);

                service.shutdown().await;
                service2.shutdown().await;

                assert_eq!(document2.text(), expected_string);

                main_loop.quit();
            }
        ));

        main_loop.run();
    }

    #[test]
    fn sync_longer_text() {
        let test_string = "Et aut omnis eos corporis ut. Qui est blanditiis blanditiis.
        Sit quia nam maxime accusantium ut voluptatem. Fuga consequuntur animi et et est.
        Unde voluptas consequatur mollitia id odit optio harum sint. Fugit quo aut et laborum aut cupiditate.";
        let expected_string = format!(
            "{}{}{}{}",
            test_string, test_string, test_string, test_string
        );

        let context = glib::MainContext::new();
        let main_loop = glib::MainLoop::new(Some(&context), false);

        context.spawn_local(clone!(
            #[strong]
            context,
            #[strong]
            main_loop,
            async move {
                let private_key = PrivateKey::new();
                let service = Service::new(&private_key, None);
                service.startup().await.unwrap();

                let document = Document::create_with_main_context(&service, &context).await;
                let id = document.id();

                document.subscribe().await;

                let private_key2 = PrivateKey::new();
                let service2 = Service::new(&private_key2, None);
                service2.startup().await.unwrap();

                let document2 = Document::with_main_context(&service2, &id, &context);
                document2.subscribe().await;

                assert_eq!(document.id(), document2.id());

                assert!(document.insert_text(0, test_string).is_ok());
                assert!(document.insert_text(0, test_string).is_ok());
                assert!(document.insert_text(0, test_string).is_ok());
                assert!(document.insert_text(0, test_string).is_ok());

                service.shutdown().await;
                service2.shutdown().await;

                assert_eq!(document2.text(), expected_string);

                main_loop.quit();
            }
        ));

        main_loop.run();
    }
}
