mod request;
use crate::model::{ManagementError, ManagementResult};
pub use request::*;

pub trait Confirmation {
    fn streams_methods(&self) -> bool {
        false
    }
    fn method_completed(
        &mut self,
        _request: &ResolvedRequest,
        _result: &crate::model::MethodResult,
    ) -> Result<(), ManagementError> {
        Ok(())
    }
    fn confirm(&mut self, prompt: &crate::model::WideText) -> Result<bool, ManagementError>;
}
pub trait Management {
    fn launch_external(
        &mut self,
        command: &crate::model::WideText,
    ) -> Result<u32, ManagementError> {
        Err(ManagementError {
            code: 0x8004100c,
            target: command.clone(),
            message: "Not supported".into(),
            operation: "external".into(),
            extended: vec![],
            partial: None,
        })
    }
    fn execute_with_confirmation(
        &mut self,
        request: &ResolvedRequest,
        _confirmation: &mut dyn Confirmation,
    ) -> Result<ManagementResult, ManagementError> {
        self.execute(request)
    }

    fn load_aliases(
        &mut self,
        _role: &crate::model::WideText,
        _selected: Option<&crate::model::WideText>,
        _locale: &crate::model::WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, crate::alias::ResolveError> {
        Ok(None)
    }
    fn reachable(&mut self, _node: &crate::model::WideText, _timeout_ms: u32) -> bool {
        true
    }
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError>;
}
mod convert;
pub use convert::*;
#[cfg(windows)]
pub mod windows;
mod wql;
pub use wql::*;

pub mod methods;

pub mod write;
