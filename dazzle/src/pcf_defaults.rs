use std::collections::HashMap;

use bytes::Buf;
use dmx::attribute::{Color, Vector3};

pub(crate) const DEFAULT_PCF_DATA: &[u8] = include_bytes!("default_values.pcf");

/// Decodes [`DEFAULT_PCF_DATA`] and produces a map of `functionName`, to a default attribute value map.
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

pub(crate) fn get_default_operator_map() -> HashMap<&'static str, pcf::Attribute> {
    HashMap::from([
        ("operator start fadein", 0.0.into()),
        ("operator end fadein", 0.0.into()),
        ("operator start fadeout", 0.0.into()),
        ("operator end fadeout", 0.0.into()),
        ("Visibility Proxy Input Control Point Number", (-1).into()),
        ("Visibility Proxy Radius", 1.0.into()),
        ("Visibility input minimum", 0.0.into()),
        ("Visibility input maximum", 1.0.into()),
        ("Visibility Alpha Scale minimum", 0.0.into()),
        ("Visibility Alpha Scale maximum", 1.0.into()),
        ("Visibility Radius Scale minimum", 1.0.into()),
        ("Visibility Radius Scale maximum", 1.0.into()),
        ("Visibility Camera Depth Bias", 0.0.into()),
    ])
}

pub(crate) fn get_particle_system_defaults() -> HashMap<&'static str, pcf::Attribute> {
    HashMap::from([
        // ("batch particle systems", false.into()),
        (
            "bounding_box_min",
            Vector3((-10.0).into(), (-10.0).into(), (-10.0).into()).into(),
        ),
        (
            "bounding_box_max",
            Vector3(10.0.into(), 10.0.into(), 10.0.into()).into(),
        ),
        ("color", Color(255, 255, 255, 255).into()),
        ("control point to disable rendering if it is the camera", (-1).into()),
        ("cull_control_point", 0.into()),
        ("cull_cost", 1.0.into()),
        ("cull_radius", 0.0.into()),
        ("cull_replacement_definition", String::new().into()),
        ("group id", 0.into()),
        ("initial_particles", 0i32.into()),
        ("max_particles", 1000i32.into()),
        ("material", "vgui/white".to_string().into()),
        ("max_particles", 1000.into()),
        ("maximum draw distance", 100_000.0.into()),
        ("maximum sim tick rate", 0.0.into()),
        ("maximum time step", 0.1.into()),
        ("minimum rendered frames", 0.into()),
        ("minimum sim tick rate", 0.0.into()),
        ("preventNameBasedLookup", false.into()),
        ("radius", 5.0.into()),
        ("rotation", 0.0.into()),
        ("rotation_speed", 0.0.into()),
        ("sequence_number", 0.into()),
        ("sequence_number1", 0.into()),
        ("Sort particles", true.into()),
        ("time to sleep when not drawn", 8.0.into()),
        ("view model effect", false.into()),
    ])
}
