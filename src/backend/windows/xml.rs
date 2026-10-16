//! Ask WMI for CIM DTD 2.0 so property origin/propagation metadata is retained.
use super::*;
use windows::{
    Win32::System::{Com::*, Variant::VARIANT},
    core::*,
};
pub(super) fn requested(r: &ResolvedRequest) -> bool {
    r.render_context.is_some()
        || r.modifiers.iter().any(|m| {
            ["format", "translate", "record"]
                .iter()
                .any(|n| m.name.eq_ascii_ignore_case(n))
        })
}
pub(super) fn text(object: &IWbemClassObject, r: &ResolvedRequest) -> Result<WideText> {
    unsafe {
        let writer: IWbemObjectTextSrc =
            CoCreateInstance(&WbemObjectTextSrc, None, CLSCTX_INPROC_SERVER)?;
        let context: IWbemContext = CoCreateInstance(&WbemContext, None, CLSCTX_INPROC_SERVER)?;
        let system = r.properties.iter().any(|p| p.0.starts_with(&[95, 95]))
            || r.display.iter().any(|(_, p)| p.0.starts_with(&[95, 95]));
        let exclude = VARIANT::from(!system);
        context.SetValue(w!("ExcludeSystemProperties"), 0, &exclude)?;
        writer
            .GetText(0, object, WMI_OBJ_TEXT_CIM_DTD_2_0.0 as u32, &context)
            .map(|text| wide(&text))
    }
}
