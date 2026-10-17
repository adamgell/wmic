use crate::{
    backend::macos::{context::Context, provider::ClassProvider},
    model::{ManagementError, Record},
};

/// A class whose schema is served and whose instance set is empty.
pub struct Unpopulated(pub &'static str);

impl ClassProvider for Unpopulated {
    fn class(&self) -> &'static str {
        self.0
    }

    fn instances(&self, _ctx: &Context) -> Result<Vec<Record>, ManagementError> {
        Ok(vec![])
    }
}
