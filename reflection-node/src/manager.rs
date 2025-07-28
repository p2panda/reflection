use p2panda_spaces::manager::Manager;
use p2panda_spaces::test_utils::MemoryStore;
use p2panda_spaces::types::StrongRemoveResolver;

use crate::forge::ReflectionForge;
use crate::operation::{ReflectionConditions, ReflectionOperation};

pub type SpacesMemoryStore = MemoryStore<
    ReflectionOperation,
    ReflectionConditions,
    StrongRemoveResolver<ReflectionConditions>,
>;

pub type ReflectionManager = Manager<
    SpacesMemoryStore,
    ReflectionForge,
    ReflectionOperation,
    ReflectionConditions,
    StrongRemoveResolver<ReflectionConditions>,
>;
