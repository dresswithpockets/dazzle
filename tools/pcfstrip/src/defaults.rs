use std::collections::HashMap;

use dmx::attribute::{Color, Vector3};

pub fn get_particle_system_defaults() -> HashMap<&'static str, pcf::Attribute> {
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
        ("material", "vgui/white".to_string().into()),
        ("max_particles", 1000i32.into()),
        ("maximum draw distance", 100_000.0.into()),
        ("maximum sim tick rate", 0.0.into()),
        ("maximum time step", 0.1.into()),
        ("minimum rendered frames", 0.into()),
        ("minimum sim tick rate", 0.0.into()),
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
