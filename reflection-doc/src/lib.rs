pub mod author;
pub mod authors;
pub mod document;
pub mod documents;
pub mod service;

pub mod identity {
    use std::fmt;
    use std::hash::Hash;

    use p2panda_core;
    pub use p2panda_core::identity::IdentityError;

    #[derive(Clone, Debug, glib::Boxed)]
    #[boxed_type(name = "ReflectionSigningKey", nullable)]
    pub struct SigningKey(pub(crate) p2panda_core::SigningKey);

    impl Default for SigningKey {
        fn default() -> Self {
            Self::generate()
        }
    }

    impl SigningKey {
        pub fn generate() -> SigningKey {
            SigningKey(p2panda_core::SigningKey::generate())
        }

        pub fn verifying_key(&self) -> VerifyingKey {
            VerifyingKey(self.0.verifying_key())
        }

        pub fn as_bytes(&self) -> &[u8] {
            self.0.as_bytes().as_slice()
        }
    }

    impl TryFrom<&[u8]> for SigningKey {
        type Error = p2panda_core::IdentityError;

        fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
            Ok(SigningKey(p2panda_core::SigningKey::try_from(value)?))
        }
    }

    impl<'a> From<&'a SigningKey> for &'a [u8] {
        fn from(value: &SigningKey) -> &[u8] {
            value.0.as_bytes().as_slice()
        }
    }

    impl fmt::Display for SigningKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, f)
        }
    }

    #[derive(Clone, Debug, PartialEq, Hash, Eq, glib::Boxed)]
    #[boxed_type(name = "ReflectionVerifyingKey", nullable)]
    pub struct VerifyingKey(pub(crate) p2panda_core::VerifyingKey);

    impl fmt::Display for VerifyingKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(&self.0, f)
        }
    }

    impl<'a> From<&'a VerifyingKey> for &'a [u8] {
        fn from(value: &VerifyingKey) -> &[u8] {
            value.0.as_bytes().as_slice()
        }
    }

    impl VerifyingKey {
        pub fn as_bytes(&self) -> &[u8] {
            self.0.as_bytes().as_slice()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::DocumentId;
    use crate::identity::SigningKey;
    use crate::service::Service;

    #[test_log::test(glib::async_test)]
    async fn create_document() {
        let test_string = "Hello World";

        let context = glib::MainContext::ref_thread_default();

        let signing_key = SigningKey::generate();
        let service = Service::new(&signing_key, None);
        service.startup().await.unwrap();

        let document = service.join_document_with_main_context(&DocumentId::new(), &context);
        document.subscribe().await;

        assert!(document.insert_text(0, test_string).is_ok());
        assert_eq!(document.text(), test_string);

        service.shutdown().await;
    }

    #[test_log::test(glib::async_test)]
    async fn basic_sync() {
        let expected_string = "Hello World";

        let context = glib::MainContext::ref_thread_default();

        let signing_key = SigningKey::generate();
        let service = Service::new(&signing_key, None);
        service.startup().await.unwrap();

        let document = service.join_document_with_main_context(&DocumentId::new(), &context);
        document.subscribe().await;
        let id = document.id();

        let signing_key2 = SigningKey::generate();
        let service2 = Service::new(&signing_key2, None);
        service2.startup().await.unwrap();

        let document2 = service2.join_document_with_main_context(&id, &context);
        document2.subscribe().await;

        assert_eq!(document.id(), document2.id());

        assert!(document.insert_text(0, expected_string).is_ok());
        assert_eq!(document.text(), expected_string);

        // Wait until text got synced.
        loop {
            glib::timeout_future(std::time::Duration::from_millis(50)).await;

            if document2.text() == expected_string {
                break;
            }
        }

        service.shutdown().await;
        service2.shutdown().await;

        assert_eq!(document2.text(), expected_string);
    }

    #[test_log::test(glib::async_test)]
    async fn sync_multiple_changes() {
        let expected_string = "Hello, World!";

        let context = glib::MainContext::ref_thread_default();

        let signing_key = SigningKey::generate();
        let service = Service::new(&signing_key, None);
        service.startup().await.unwrap();

        let document = service.join_document_with_main_context(&DocumentId::new(), &context);
        document.subscribe().await;
        let id = document.id();

        let signing_key2 = SigningKey::generate();
        let service2 = Service::new(&signing_key2, None);
        service2.startup().await.unwrap();

        let document2 = service2.join_document_with_main_context(&id, &context);
        document2.subscribe().await;

        assert_eq!(document.id(), document2.id());

        assert!(document.insert_text(0, "Hello,").is_ok());
        assert!(document.insert_text(6, " World!").is_ok());
        assert!(document.delete_range(7, 8).is_ok());
        assert!(document.insert_text(7, "W").is_ok());
        assert_eq!(document.text(), expected_string);

        // Wait until text got synced.
        loop {
            glib::timeout_future(std::time::Duration::from_millis(50)).await;

            if document2.text() == expected_string {
                break;
            }
        }

        service.shutdown().await;
        service2.shutdown().await;

        assert_eq!(document2.text(), expected_string);
    }

    #[test_log::test(glib::async_test)]
    async fn sync_longer_text() {
        let test_string = "Et aut omnis eos corporis ut. Qui est blanditiis blanditiis. Sit quia
        nam maxime accusantium ut voluptatem. Fuga consequuntur animi et et est. Unde voluptas
        consequatur mollitia id odit optio harum sint. Fugit quo aut et laborum aut cupiditate.";

        let expected_string = format!(
            "{}{}{}{}",
            test_string, test_string, test_string, test_string
        );

        let context = glib::MainContext::ref_thread_default();

        let signing_key = SigningKey::generate();
        let service = Service::new(&signing_key, None);
        service.startup().await.unwrap();

        let document = service.join_document_with_main_context(&DocumentId::new(), &context);
        let id = document.id();

        document.subscribe().await;

        let signing_key2 = SigningKey::generate();
        let service2 = Service::new(&signing_key2, None);
        service2.startup().await.unwrap();

        let document2 = service2.join_document_with_main_context(&id, &context);
        document2.subscribe().await;

        assert_eq!(document.id(), document2.id());

        assert!(document.insert_text(0, test_string).is_ok());
        assert!(document.insert_text(0, test_string).is_ok());
        assert!(document.insert_text(0, test_string).is_ok());
        assert!(document.insert_text(0, test_string).is_ok());

        // Wait until text got synced.
        loop {
            glib::timeout_future(std::time::Duration::from_millis(50)).await;

            if document2.text() == expected_string {
                break;
            }
        }

        service.shutdown().await;
        service2.shutdown().await;

        assert_eq!(document2.text(), expected_string);
    }
}
