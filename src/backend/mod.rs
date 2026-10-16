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
    fn load_translation(
        &mut self,
        _role: &crate::model::WideText,
        _name: &crate::model::WideText,
    ) -> Result<Option<crate::render::TranslationEntries>, crate::alias::ResolveError> {
        Ok(None)
    }
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

    /// Names and descriptions for global help; targeted commands load full metadata.
    fn load_alias_inventory(
        &mut self,
        role: &crate::model::WideText,
        locale: &crate::model::WideText,
    ) -> Result<Option<Vec<crate::alias::AliasDefinition>>, crate::alias::ResolveError> {
        self.load_aliases(role, None, locale)
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
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(windows))]
pub mod wsman;
#[cfg(windows)]
pub mod windows;
mod wql;
pub use wql::*;

pub mod methods;

pub mod write;

/// Check cancellation lazily before handing each target to a mutation loop.
pub fn cancellable_targets<T>(
    targets: impl IntoIterator<Item = T>,
    mut cancelled: impl FnMut() -> bool,
) -> impl Iterator<Item = Result<T, u32>> {
    let mut stopped = false;
    targets.into_iter().map_while(move |target| {
        if stopped {
            return None;
        }
        if cancelled() {
            stopped = true;
            Some(Err(0xC000013A))
        } else {
            Some(Ok(target))
        }
    })
}
/// Association errors must carry both completed sources and the current partial group.
pub fn collect_associations(
    sources: Result<crate::model::ObjectSet, ManagementError>,
    mut query: impl FnMut(&crate::model::Record) -> Result<crate::model::ObjectSet, ManagementError>,
) -> Result<Vec<crate::model::AssociationGroup>, ManagementError> {
    use crate::model::*;
    let (sources, mut source_error) = match sources {
        Ok(set) => (set, None),
        Err(mut error) => {
            let set = match error.partial.take().map(|p| *p) {
                Some(ManagementResult::Objects(set)) => set,
                _ => ObjectSet::default(),
            };
            (set, Some(error))
        }
    };
    let mut groups = vec![];
    for source in sources.objects {
        match query(&source) {
            Ok(set) => groups.push(AssociationGroup {
                source,
                associated: set.objects,
            }),
            Err(mut error) => {
                if let Some(ManagementResult::Objects(set)) = error.partial.take().map(|p| *p)
                    && !set.objects.is_empty()
                {
                    groups.push(AssociationGroup {
                        source,
                        associated: set.objects,
                    });
                }
                if !groups.is_empty() {
                    error.partial = Some(Box::new(ManagementResult::Associations(groups)));
                }
                return Err(error);
            }
        }
    }
    if let Some(mut error) = source_error.take() {
        if !groups.is_empty() {
            error.partial = Some(Box::new(ManagementResult::Associations(groups)));
        }
        Err(error)
    } else {
        Ok(groups)
    }
}
