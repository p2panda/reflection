pub mod author;
pub mod authors;
pub mod document;
pub mod documents;
pub mod service;

pub mod identity {
    use reflection_node::p2panda_core;
    pub use reflection_node::p2panda_core::identity::IdentityError;
    use std::{fmt, str::FromStr};

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

    impl FromStr for PrivateKey {
        type Err = p2panda_core::IdentityError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let bytes = hex::decode(s)?;
            let len = bytes.len();
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| IdentityError::InvalidLength(len, 32))?;
            Ok(Self(p2panda_core::PrivateKey::from_bytes(&bytes)))
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
    use crate::document::Document;
    use crate::identity::PrivateKey;
    use crate::service::Service;
    use gio::prelude::FileExt;
    use glib::object::ObjectExt;
    use std::fs;
    use test_log::test;

    struct TestResource {
        service: Service,
    }

    impl Drop for TestResource {
        fn drop(&mut self) {
            if let Some(data_dir) = self.service.data_dir() {
                fs::remove_dir_all(data_dir.path().unwrap()).expect("Able to remove data dir");
            }
        }
    }

    impl TestResource {
        /// Creates a new `TestResource` that includes the `Service`
        fn new() -> TestResource {
            let private_key = PrivateKey::new();
            let mut data_path = glib::tmp_dir();
            data_path.push("Reflection");
            data_path.push(private_key.public_key().to_string());
            fs::create_dir_all(&data_path).expect("Able to create data dir");
            let data_dir = gio::File::for_path(data_path);

            TestResource {
                service: Service::new(&private_key, Some(&data_dir)),
            }
        }

        fn service(&self) -> Service {
            self.service.clone()
        }
    }

    #[test]
    fn create_document() {
        let context = glib::MainContext::default();

        let resource = TestResource::new();
        let service = resource.service();
        let test_string = "Hello World";
        glib::MainContext::new()
            .block_on(async { service.startup().await })
            .unwrap();
        let document = Document::new(&service, None);
        document.set_subscribed(true);
        context.iteration(false);
        assert!(document.insert_text(0, test_string).is_ok());
        assert_eq!(document.text(), test_string);
    }

    #[test]
    fn basic_sync() {
        let main_loop = glib::MainLoop::new(None, false);
        let test_string = "Hello World";
        let resource = TestResource::new();
        let service = resource.service();
        glib::MainContext::new()
            .block_on(async { service.startup().await })
            .unwrap();

        let document = Document::new(&service, None);
        document.set_subscribed(true);
        let id = document.id();

        let resource2 = TestResource::new();
        let service2 = resource2.service();
        glib::MainContext::new()
            .block_on(async { service2.startup().await })
            .unwrap();
        let document2 = Document::new(&service2, Some(&id));
        document2.set_subscribed(true);

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
        let resource = TestResource::new();
        let service = resource.service();
        glib::MainContext::new()
            .block_on(async { service.startup().await })
            .unwrap();

        let document = Document::new(&service, None);
        document.set_subscribed(true);
        let id = document.id();

        let resource2 = TestResource::new();
        let service2 = resource2.service();
        glib::MainContext::new()
            .block_on(async { service2.startup().await })
            .unwrap();
        let document2 = Document::new(&service2, Some(&id));
        document2.set_subscribed(true);

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
        let resource = TestResource::new();
        let service = resource.service();
        glib::MainContext::new()
            .block_on(async { service.startup().await })
            .unwrap();

        let document = Document::new(&service, None);
        document.set_subscribed(true);
        let id = document.id();

        let resource2 = TestResource::new();
        let service2 = resource2.service();
        glib::MainContext::new()
            .block_on(async { service2.startup().await })
            .unwrap();
        let document2 = Document::new(&service2, Some(&id));
        document2.set_subscribed(true);

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
