use super::*;
pub fn table(objects: &[Record], cols: &[(WideText, WideText)]) -> WideText {
    let rows: Vec<Vec<WideText>> = objects
        .iter()
        .map(|r| {
            cols.iter()
                .map(|(_, source)| {
                    let mut text = lookup(r, source)
                        .map(value::raw_display_value)
                        .unwrap_or_default();
                    text.0.retain(|u| *u != 13 && *u != 10);
                    text
                })
                .collect()
        })
        .collect();
    let widths: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            std::iter::once(width(name))
                .chain(rows.iter().map(|r| width(&r[i])))
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
                widths[i].saturating_sub(width(cell)),
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

fn width(text: &WideText) -> usize {
    text.0.iter().map(|u| if *u > 255 { 2 } else { 1 }).sum()
}
