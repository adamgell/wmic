use super::*;

fn is_system(name: &WideText) -> bool {
    name.0.starts_with(&[95, 95])
}

fn order(name: &WideText) -> (usize, String) {
    (
        system_property_order()
            .iter()
            .position(|n| name.eq_ignore_case(n))
            .unwrap_or(10),
        name.to_string_lossy().to_ascii_lowercase(),
    )
}

/// Records without captured WMI text carry every property they were built with. The captured
/// text of the Windows backend holds only the requested properties, plus all system properties
/// when one of them is requested. The selection and order of the direct renderers is applied
/// before the command XML is written.
pub(crate) fn selected(req: &ResolvedRequest, set: &ObjectSet) -> ObjectSet {
    let wildcard = req.verb == Verb::Get && req.properties.iter().any(|name| name.0 == [42]);
    let cols = if wildcard {
        let mut cols: Vec<_> = set
            .objects
            .first()
            .map(|r| {
                r.properties
                    .iter()
                    .filter(|p| !p.system && !is_system(&p.name))
                    .map(|p| (p.name.clone(), p.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        cols.extend(
            req.properties
                .iter()
                .filter(|name| is_system(name))
                .map(|name| (name.clone(), name.clone())),
        );
        cols
    } else {
        columns(req, &set.objects)
    };
    let system_requested = cols.iter().any(|(_, source)| is_system(source));
    ObjectSet {
        objects: set
            .objects
            .iter()
            .map(|record| {
                if record.wmi_xml.is_some() {
                    return record.clone();
                }
                let mut picked = record.clone();
                picked.properties = cols
                    .iter()
                    .filter_map(|(_, source)| {
                        record
                            .properties
                            .iter()
                            .find(|p| p.name.eq_ignore_case(&source.to_string_lossy()))
                    })
                    .chain(
                        record
                            .properties
                            .iter()
                            .filter(|p| system_requested && is_system(&p.name)),
                    )
                    .fold(Vec::<Property>::new(), |mut kept, p| {
                        if !kept.iter().any(|k| k.name == p.name) {
                            kept.push(p.clone())
                        }
                        kept
                    });
                picked.properties.sort_by_key(|p| order(&p.name));
                picked
            })
            .collect(),
        schema: set.schema.clone(),
    }
}
