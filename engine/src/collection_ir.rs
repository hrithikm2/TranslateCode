//! Canonical standard-library collection vocabulary shared by every frontend.
//!
//! Source imports and concrete implementations are intentionally erased here. Backends choose
//! the native target representation from the canonical family name.

pub const LIST: &str = "List";
pub const MAP: &str = "Map";
pub const SET: &str = "Set";
pub const QUEUE: &str = "Queue";

use crate::typed_ir::IntrinsicOperation;

pub fn intrinsic_for_method(name: &str, argument_count: usize) -> Option<IntrinsicOperation> {
    match (name, argument_count) {
        ("containsKey" | "contains_key", 1) => Some(IntrinsicOperation::MapContainsKey),
        ("containsValue" | "contains_value", 1) => Some(IntrinsicOperation::MapContainsValue),
        ("contains" | "includes" | "has", 1) => Some(IntrinsicOperation::CollectionContains),
        ("indexOf" | "index_of", 1 | 2) => Some(IntrinsicOperation::CollectionIndexOf),
        ("slice" | "subList" | "sublist", 1 | 2) => Some(IntrinsicOperation::CollectionSlice),
        ("clear", 0) => Some(IntrinsicOperation::CollectionClear),
        ("add" | "append", 1) => Some(IntrinsicOperation::CollectionAdd),
        ("addAll" | "extend", 1) => Some(IntrinsicOperation::CollectionAddAll),
        ("remove", 1) => Some(IntrinsicOperation::CollectionRemove),
        ("removeAt", 1) => Some(IntrinsicOperation::CollectionRemoveAt),
        ("addFirst" | "appendleft" | "push_front", 1) => Some(IntrinsicOperation::QueueAddFirst),
        ("addLast" | "push_back", 1) => Some(IntrinsicOperation::QueueAddLast),
        ("removeFirst" | "popleft" | "pop_front", 0) => Some(IntrinsicOperation::QueueRemoveFirst),
        ("removeLast" | "pop_back", 0) => Some(IntrinsicOperation::QueueRemoveLast),
        _ => None,
    }
}

pub fn canonical_collection_type(name: &str) -> Option<&'static str> {
    let leaf = name
        .rsplit("::")
        .next()
        .unwrap_or(name)
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .trim();
    match leaf {
        "Array"
        | "ArrayList"
        | "CopyOnWriteArrayList"
        | "List"
        | "MutableList"
        | "Vector"
        | "Vec"
        | "list" => Some(LIST),
        "Dictionary" | "HashMap" | "LinkedHashMap" | "SplayTreeMap" | "TreeMap" | "BTreeMap"
        | "ConcurrentHashMap" | "ConcurrentMap" | "Map" | "dict" | "defaultdict"
        | "OrderedDict" | "map" => Some(MAP),
        "HashSet" | "LinkedHashSet" | "SplayTreeSet" | "TreeSet" | "BTreeSet" | "Set" | "set"
        | "frozenset" => Some(SET),
        "ArrayDeque" | "Deque" | "DoubleLinkedQueue" | "ListQueue" | "Queue" | "VecDeque"
        | "deque" => Some(QUEUE),
        _ => None,
    }
}

pub fn is_standard_collection_import(uri: &str) -> bool {
    let uri = uri.trim().trim_end_matches(".*");
    uri == "dart:collection"
        || uri == "java.util"
        || uri.starts_with("java.util.")
        || uri == "collections"
        || uri == "collections.abc"
        || uri == "typing"
        || uri == "Foundation"
        || uri == "container/list"
        || uri == "container/heap"
        || uri == "container/ring"
        || uri == "std::collections"
        || uri.starts_with("std::collections::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_concrete_collection_types_from_all_standard_families() {
        for name in ["ArrayList", "List", "Vec", "list", "Array"] {
            assert_eq!(canonical_collection_type(name), Some(LIST), "{name}");
        }
        for name in [
            "HashMap",
            "LinkedHashMap",
            "SplayTreeMap",
            "TreeMap",
            "BTreeMap",
            "dict",
            "Dictionary",
            "map",
        ] {
            assert_eq!(canonical_collection_type(name), Some(MAP), "{name}");
        }
        for name in [
            "HashSet",
            "LinkedHashSet",
            "TreeSet",
            "BTreeSet",
            "set",
            "Set",
        ] {
            assert_eq!(canonical_collection_type(name), Some(SET), "{name}");
        }
        for name in ["ArrayDeque", "ListQueue", "VecDeque", "deque", "Queue"] {
            assert_eq!(canonical_collection_type(name), Some(QUEUE), "{name}");
        }
    }

    #[test]
    fn recognizes_standard_collection_imports_without_treating_packages_as_standard() {
        for uri in [
            "dart:collection",
            "java.util.HashMap",
            "collections",
            "collections.abc",
            "typing",
            "Foundation",
            "container/list",
            "std::collections",
        ] {
            assert!(is_standard_collection_import(uri), "{uri}");
        }
        assert!(!is_standard_collection_import(
            "package:collection/collection.dart"
        ));
        assert!(!is_standard_collection_import("com.example.collections"));
    }
}
