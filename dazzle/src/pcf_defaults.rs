use std::collections::HashMap;

use bytes::Buf;

// N.B. get_default_attribute_map and DEFAULT_PCF_DATA is an experiment to trim all possible default attribute values.
//      atm build.rs trims a static list of attribute defaults that have been shown to work experimentally.

#[allow(dead_code)]
pub(crate) const DEFAULT_PCF_DATA: &[u8] = include_bytes!("static/default_values.pcf");

/// Decodes [`DEFAULT_PCF_DATA`] and produces a map of `functionName`, to a default attribute value map.
#[allow(dead_code)]
pub(crate) fn get_default_attribute_map() -> anyhow::Result<HashMap<String, HashMap<String, pcf::Attribute>>> {
    let mut reader = DEFAULT_PCF_DATA.reader();
    let dmx = dmx::decode(&mut reader)?;
    let pcf = pcf::new::Pcf::try_from(dmx)?;

    let (_, symbols, root) = pcf.into_parts();
    let (_, _, particle_systems, _) = root.into_parts();

    let all_operators = particle_systems
        .into_iter()
        .flat_map(|system| {
            [
                system.constraints,
                system.emitters,
                system.forces,
                system.initializers,
                system.operators,
                system.renderers,
            ]
        })
        .flatten();

    let mut operator_map = HashMap::new();
    for operator in all_operators {
        let value_map: HashMap<_, _> = operator
            .attributes
            .into_iter()
            // .filter(|(name_idx, _)| {
            //     symbols.base.get_index_of("distance_bias").is_none_or(|idx| *name_idx as usize != idx)
            // })
            .map(|(name_idx, attribute)| {
                let name = symbols
                    .base
                    .get_index(name_idx as usize)
                    .expect("this should never happen");
                (name.clone(), attribute)
            })
            .collect();

        operator_map.insert(operator.function_name, value_map);
    }

    Ok(operator_map)
}
