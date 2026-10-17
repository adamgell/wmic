//! Providers enumerate instances of one class; the registry pairs each with its schema.
use super::{context::Context, providers, schema};
use crate::model::{CimValue, ManagementError, MethodSchema, Property, Record, Verb, WideText};
use std::{collections::HashMap, sync::OnceLock};

/// One method invocation on one target.
pub struct MethodCall<'a> {
    pub method: &'a MethodSchema,
    /// Object path of the instance, or the class name for a static method.
    pub path: &'a WideText,
    /// The instance; `None` for static methods and class targets.
    pub instance: Option<&'a Record>,
    /// Arguments bound against the method's input parameters.
    pub input: &'a [Property],
}

/// Values a method returns.
#[derive(Default)]
pub struct MethodOutput {
    pub return_value: Option<CimValue>,
    pub out_parameters: Vec<Property>,
}

/// One create, update or delete of one target.
pub struct Change<'a> {
    pub verb: Verb,
    pub path: Option<&'a WideText>,
    pub instance: Option<&'a Record>,
    pub properties: &'a [Property],
}

pub trait ClassProvider: Send + Sync {
    /// Canonical class name; it must exist in the embedded schema.
    fn class(&self) -> &'static str;

    /// Every instance of the class, with all schema properties present.
    fn instances(&self, ctx: &Context) -> Result<Vec<Record>, ManagementError>;

    /// Names of methods `call` executes; the schema reports them as implemented.
    fn implemented_methods(&self) -> &'static [&'static str] {
        &[]
    }

    fn call(&self, ctx: &Context, _call: &MethodCall<'_>) -> Result<MethodOutput, ManagementError> {
        Err(ctx.not_supported("method"))
    }

    fn write(&self, ctx: &Context, _change: &Change<'_>) -> Result<(), ManagementError> {
        Err(ctx.not_supported("write"))
    }
}

pub struct Entry {
    pub provider: Box<dyn ClassProvider>,
    pub info: schema::ClassInfo,
}

pub struct Registry {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
}

impl Registry {
    /// Case-insensitive class lookup.
    pub fn get(&self, class: &str) -> Option<&Entry> {
        self.index
            .get(&class.to_ascii_lowercase())
            .map(|i| &self.entries[*i])
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

fn build() -> Registry {
    let mut all = providers::all();
    for name in providers::EMPTY_CLASSES {
        if !all.iter().any(|p| p.class().eq_ignore_ascii_case(name)) {
            all.push(Box::new(providers::empty::Unpopulated(name)));
        }
    }
    let mut entries = vec![];
    let mut index = HashMap::new();
    for provider in all {
        let name = provider.class();
        let info = schema::load(name, provider.implemented_methods())
            .unwrap_or_else(|| panic!("class {name} is missing from the embedded schema"));
        let key = name.to_ascii_lowercase();
        assert!(!index.contains_key(&key), "class {name} has two providers");
        index.insert(key, entries.len());
        entries.push(Entry { provider, info });
    }
    Registry { entries, index }
}

pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(build)
}
