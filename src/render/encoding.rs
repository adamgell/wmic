use crate::model::WideText;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestinationProfile {
    pub code_page: u32,
    pub utf16: bool,
    pub bom: bool,
}
impl DestinationProfile {
    pub fn pipe(code_page: u32) -> Self {
        Self {
            code_page,
            utf16: false,
            bom: false,
        }
    }
    pub fn utf16_file(bom: bool) -> Self {
        Self {
            code_page: 1200,
            utf16: true,
            bom,
        }
    }
}
#[derive(Debug)]
pub struct OutputError(pub String);
impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for OutputError {}
pub fn encode_output(
    text: &WideText,
    profile: &DestinationProfile,
) -> Result<Vec<u8>, OutputError> {
    if profile.utf16 {
        let mut out = if profile.bom { vec![255, 254] } else { vec![] };
        out.extend(text.0.iter().flat_map(|u| u.to_le_bytes()));
        return Ok(out);
    }
    if profile.code_page == 65001 {
        return Ok(String::from_utf16_lossy(&text.0).into_bytes());
    }
    #[cfg(windows)]
    {
        crate::platform::windows::encoding::encode_codepage(text, profile.code_page)
    }
    #[cfg(not(windows))]
    {
        if profile.code_page != 1252 {
            return Err(OutputError("This code page requires Windows".into()));
        }
        let mut out = vec![];
        for unit in &text.0 {
            out.push(if *unit < 128 || (160..=255).contains(unit) {
                *unit as u8
            } else {
                let special = [
                    0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030,
                    0x0160, 0x2039, 0x0152, 0x008d, 0x017d, 0x008f, 0x0090, 0x2018, 0x2019, 0x201c,
                    0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x009d,
                    0x017e, 0x0178,
                ];
                special
                    .iter()
                    .position(|v| v == unit)
                    .map(|i| i as u8 + 128)
                    .unwrap_or(b'?')
            });
        }
        Ok(out)
    }
}
