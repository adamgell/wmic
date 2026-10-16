//! Class definitions from the embedded schema, shared by record typing and schema requests.
use super::schema::{ClassInfo, load};
use crate::model::CimValue;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

type Cache = Mutex<HashMap<String, Option<Arc<ClassInfo>>>>;

/// Case-insensitive lookup. Methods report as implemented because the remote host serves them.
pub fn lookup(class: &str) -> Option<Arc<ClassInfo>> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Cache::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .entry(class.to_ascii_lowercase())
        .or_insert_with(|| {
            let mut info = load(class, &[])?;
            for method in &mut info.schema.methods {
                for qualifier in &mut method.qualifiers {
                    if qualifier.name.eq_ignore_case("implemented") {
                        qualifier.value = CimValue::Boolean(true);
                    }
                }
            }
            Some(Arc::new(info))
        })
        .clone()
}
