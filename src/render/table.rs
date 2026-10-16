use super::*;
pub fn table(objects: &[Record], cols: &[(WideText, WideText)]) -> WideText {
    let rows: Vec<Vec<WideText>> = objects
        .iter()
        .map(|r| {
            cols.iter()
                .map(|(_, source)| lookup(r, source).map(display_value).unwrap_or_default())
                .collect()
        })
        .collect();
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            std::iter::once(name.0.len())
                .chain(rows.iter().map(|r| r[i].0.len()))
                .max()
                .unwrap_or(0)
                + 2
        })
        .collect();
    let mut out = WideText::default();
    let mut row = |cells: &[WideText]| {
        for (i, cell) in cells.iter().enumerate() {
            append(&mut out, cell);
            out.0.extend(std::iter::repeat_n(
                32,
                widths[i].saturating_sub(cell.0.len()),
            ));
        }
        push(&mut out, NL);
    };
    row(&cols
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>());
    for cells in rows {
        row(&cells);
    }
    push(&mut out, NL);
    out
}
