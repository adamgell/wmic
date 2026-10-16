use crate::{
    backend::{Management, ResolvedRequest},
    model::{ManagementError, ManagementResult},
};
pub struct MacManagement;
impl MacManagement {
    pub fn new() -> Result<Self, ManagementError> {
        Ok(Self)
    }
}
impl Management for MacManagement {
    fn execute(&mut self, request: &ResolvedRequest) -> Result<ManagementResult, ManagementError> {
        Err(ManagementError {
            code: 0x80041010,
            target: request.connection.node.clone(),
            message: "Invalid class".into(),
            operation: "query".into(),
            extended: vec![],
            partial: None,
        })
    }
}
