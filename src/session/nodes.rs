use crate::model::WideText;
#[derive(Debug)]
pub struct InputError {
    pub code: u32,
    pub message: WideText,
}
pub fn load_nodes(value: &WideText) -> Result<Vec<WideText>, InputError> {
    let mut nodes = vec![];
    for part in value.0.split(|u| *u == 44) {
        let mut part = WideText(part.to_vec()).trim_ascii();
        if part.0.first() == Some(&34) && part.0.last() == Some(&34) && part.0.len() >= 2 {
            part.0 = part.0[1..part.0.len() - 1].to_vec()
        }
        if part.0.first() == Some(&64) {
            let mut path = WideText(part.0[1..].to_vec());
            if path.0.first() == Some(&34) && path.0.last() == Some(&34) && path.0.len() >= 2 {
                path.0 = path.0[1..path.0.len() - 1].to_vec();
            }
            let data = read_file(&path)?;
            let decoded = if data.starts_with(&[255, 254]) {
                WideText(
                    data[2..]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect(),
                )
            } else if data.starts_with(&[254, 255]) {
                WideText(
                    data[2..]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect(),
                )
            } else {
                String::from_utf8(data)
                    .map(WideText::from)
                    .map_err(|_| InputError {
                        code: 0x80070057,
                        message: "Invalid node file encoding.".into(),
                    })?
            };
            for line in decoded.0.split(|u| *u == 10 || *u == 13) {
                let node = WideText(line.to_vec()).trim_ascii();
                insert(&mut nodes, node);
            }
        } else {
            insert(&mut nodes, part);
        }
    }
    Ok(nodes)
}
fn insert(nodes: &mut Vec<WideText>, node: WideText) {
    if !node.0.is_empty()
        && !nodes
            .iter()
            .any(|n| n.eq_ascii_ignore_case(&node.to_string_lossy()))
    {
        nodes.push(node)
    }
}
fn read_file(path: &WideText) -> Result<Vec<u8>, InputError> {
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt;
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&path.0))
    };
    #[cfg(not(windows))]
    let path = std::path::PathBuf::from(String::from_utf16(&path.0).map_err(|_| InputError {
        code: 0x80070057,
        message: "Invalid file name.".into(),
    })?);
    std::fs::read(path).map_err(|e| InputError {
        code: 0x80070000 | e.raw_os_error().unwrap_or(2) as u32,
        message: "Unable to read node list file.".into(),
    })
}
