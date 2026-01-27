include!(concat!(env!("OUT_DIR"), "/particles_manifest.rs"));

// TODO: generate this in build.rs instead of hardcoding each path
pub const PARTICLES_BYTES: [(&str, &[u8]); 102] = [
    (
        "particles/rockettrail.pcf",
        include_bytes!("../vanilla/particles/rockettrail.pcf"),
    ),
    (
        "particles/teleport_status.pcf",
        include_bytes!("../vanilla/particles/teleport_status.pcf"),
    ),
    (
        "particles/explosion.pcf",
        include_bytes!("../vanilla/particles/explosion.pcf"),
    ),
    (
        "particles/player_recent_teleport.pcf",
        include_bytes!("../vanilla/particles/player_recent_teleport.pcf"),
    ),
    (
        "particles/rocketjumptrail.pcf",
        include_bytes!("../vanilla/particles/rocketjumptrail.pcf"),
    ),
    (
        "particles/rocketbackblast.pcf",
        include_bytes!("../vanilla/particles/rocketbackblast.pcf"),
    ),
    (
        "particles/flamethrower.pcf",
        include_bytes!("../vanilla/particles/flamethrower.pcf"),
    ),
    (
        "particles/flamethrower_mvm.pcf",
        include_bytes!("../vanilla/particles/flamethrower_mvm.pcf"),
    ),
    (
        "particles/burningplayer.pcf",
        include_bytes!("../vanilla/particles/burningplayer.pcf"),
    ),
    (
        "particles/blood_impact.pcf",
        include_bytes!("../vanilla/particles/blood_impact.pcf"),
    ),
    (
        "particles/blood_trail.pcf",
        include_bytes!("../vanilla/particles/blood_trail.pcf"),
    ),
    (
        "particles/muzzle_flash.pcf",
        include_bytes!("../vanilla/particles/muzzle_flash.pcf"),
    ),
    (
        "particles/teleported_fx.pcf",
        include_bytes!("../vanilla/particles/teleported_fx.pcf"),
    ),
    (
        "particles/cig_smoke.pcf",
        include_bytes!("../vanilla/particles/cig_smoke.pcf"),
    ),
    ("particles/crit.pcf", include_bytes!("../vanilla/particles/crit.pcf")),
    (
        "particles/medicgun_beam.pcf",
        include_bytes!("../vanilla/particles/medicgun_beam.pcf"),
    ),
    (
        "particles/bigboom.pcf",
        include_bytes!("../vanilla/particles/bigboom.pcf"),
    ),
    ("particles/water.pcf", include_bytes!("../vanilla/particles/water.pcf")),
    (
        "particles/stickybomb.pcf",
        include_bytes!("../vanilla/particles/stickybomb.pcf"),
    ),
    (
        "particles/buildingdamage.pcf",
        include_bytes!("../vanilla/particles/buildingdamage.pcf"),
    ),
    (
        "particles/nailtrails.pcf",
        include_bytes!("../vanilla/particles/nailtrails.pcf"),
    ),
    (
        "particles/speechbubbles.pcf",
        include_bytes!("../vanilla/particles/speechbubbles.pcf"),
    ),
    (
        "particles/bullet_tracers.pcf",
        include_bytes!("../vanilla/particles/bullet_tracers.pcf"),
    ),
    (
        "particles/nemesis.pcf",
        include_bytes!("../vanilla/particles/nemesis.pcf"),
    ),
    (
        "particles/disguise.pcf",
        include_bytes!("../vanilla/particles/disguise.pcf"),
    ),
    (
        "particles/sparks.pcf",
        include_bytes!("../vanilla/particles/sparks.pcf"),
    ),
    (
        "particles/flag_particles.pcf",
        include_bytes!("../vanilla/particles/flag_particles.pcf"),
    ),
    (
        "particles/buildingdamage.pcf",
        include_bytes!("../vanilla/particles/buildingdamage.pcf"),
    ),
    (
        "particles/shellejection.pcf",
        include_bytes!("../vanilla/particles/shellejection.pcf"),
    ),
    (
        "particles/medicgun_attrib.pcf",
        include_bytes!("../vanilla/particles/medicgun_attrib.pcf"),
    ),
    (
        "particles/item_fx.pcf",
        include_bytes!("../vanilla/particles/item_fx.pcf"),
    ),
    (
        "particles/cinefx.pcf",
        include_bytes!("../vanilla/particles/cinefx.pcf"),
    ),
    (
        "particles/impact_fx.pcf",
        include_bytes!("../vanilla/particles/impact_fx.pcf"),
    ),
    (
        "particles/conc_stars.pcf",
        include_bytes!("../vanilla/particles/conc_stars.pcf"),
    ),
    (
        "particles/class_fx.pcf",
        include_bytes!("../vanilla/particles/class_fx.pcf"),
    ),
    (
        "particles/dirty_explode.pcf",
        include_bytes!("../vanilla/particles/dirty_explode.pcf"),
    ),
    (
        "particles/smoke_blackbillow_hoodoo.pcf",
        include_bytes!("../vanilla/particles/smoke_blackbillow_hoodoo.pcf"),
    ),
    (
        "particles/scary_ghost.pcf",
        include_bytes!("../vanilla/particles/scary_ghost.pcf"),
    ),
    (
        "particles/soldierbuff.pcf",
        include_bytes!("../vanilla/particles/soldierbuff.pcf"),
    ),
    (
        "particles/training.pcf",
        include_bytes!("../vanilla/particles/training.pcf"),
    ),
    (
        "particles/stormfront.pcf",
        include_bytes!("../vanilla/particles/stormfront.pcf"),
    ),
    (
        "particles/coin_spin.pcf",
        include_bytes!("../vanilla/particles/coin_spin.pcf"),
    ),
    (
        "particles/stamp_spin.pcf",
        include_bytes!("../vanilla/particles/stamp_spin.pcf"),
    ),
    (
        "particles/rain_custom.pcf",
        include_bytes!("../vanilla/particles/rain_custom.pcf"),
    ),
    (
        "particles/npc_fx.pcf",
        include_bytes!("../vanilla/particles/npc_fx.pcf"),
    ),
    (
        "particles/drg_cowmangler.pcf",
        include_bytes!("../vanilla/particles/drg_cowmangler.pcf"),
    ),
    (
        "particles/drg_bison.pcf",
        include_bytes!("../vanilla/particles/drg_bison.pcf"),
    ),
    (
        "particles/dxhr_fx.pcf",
        include_bytes!("../vanilla/particles/dxhr_fx.pcf"),
    ),
    (
        "particles/eyeboss.pcf",
        include_bytes!("../vanilla/particles/eyeboss.pcf"),
    ),
    (
        "particles/bombinomicon.pcf",
        include_bytes!("../vanilla/particles/bombinomicon.pcf"),
    ),
    (
        "particles/harbor_fx.pcf",
        include_bytes!("../vanilla/particles/harbor_fx.pcf"),
    ),
    (
        "particles/drg_engineer.pcf",
        include_bytes!("../vanilla/particles/drg_engineer.pcf"),
    ),
    (
        "particles/drg_pyro.pcf",
        include_bytes!("../vanilla/particles/drg_pyro.pcf"),
    ),
    ("particles/xms.pcf", include_bytes!("../vanilla/particles/xms.pcf")),
    ("particles/mvm.pcf", include_bytes!("../vanilla/particles/mvm.pcf")),
    (
        "particles/doomsday_fx.pcf",
        include_bytes!("../vanilla/particles/doomsday_fx.pcf"),
    ),
    (
        "particles/halloween.pcf",
        include_bytes!("../vanilla/particles/halloween.pcf"),
    ),
    (
        "particles/items_demo.pcf",
        include_bytes!("../vanilla/particles/items_demo.pcf"),
    ),
    (
        "particles/items_engineer.pcf",
        include_bytes!("../vanilla/particles/items_engineer.pcf"),
    ),
    (
        "particles/bl_killtaunt.pcf",
        include_bytes!("../vanilla/particles/bl_killtaunt.pcf"),
    ),
    (
        "particles/urban_fx.pcf",
        include_bytes!("../vanilla/particles/urban_fx.pcf"),
    ),
    (
        "particles/killstreak.pcf",
        include_bytes!("../vanilla/particles/killstreak.pcf"),
    ),
    (
        "particles/taunt_fx.pcf",
        include_bytes!("../vanilla/particles/taunt_fx.pcf"),
    ),
    ("particles/rps.pcf", include_bytes!("../vanilla/particles/rps.pcf")),
    (
        "particles/firstperson_weapon_fx.pcf",
        include_bytes!("../vanilla/particles/firstperson_weapon_fx.pcf"),
    ),
    (
        "particles/powerups.pcf",
        include_bytes!("../vanilla/particles/powerups.pcf"),
    ),
    (
        "particles/weapon_unusual_isotope.pcf",
        include_bytes!("../vanilla/particles/weapon_unusual_isotope.pcf"),
    ),
    (
        "particles/weapon_unusual_hot.pcf",
        include_bytes!("../vanilla/particles/weapon_unusual_hot.pcf"),
    ),
    (
        "particles/weapon_unusual_cool.pcf",
        include_bytes!("../vanilla/particles/weapon_unusual_cool.pcf"),
    ),
    (
        "particles/weapon_unusual_energyorb.pcf",
        include_bytes!("../vanilla/particles/weapon_unusual_energyorb.pcf"),
    ),
    (
        "particles/passtime.pcf",
        include_bytes!("../vanilla/particles/passtime.pcf"),
    ),
    (
        "particles/passtime_beam.pcf",
        include_bytes!("../vanilla/particles/passtime_beam.pcf"),
    ),
    (
        "particles/passtime_tv_projection.pcf",
        include_bytes!("../vanilla/particles/passtime_tv_projection.pcf"),
    ),
    (
        "particles/vgui_menu_particles.pcf",
        include_bytes!("../vanilla/particles/vgui_menu_particles.pcf"),
    ),
    (
        "particles/invasion_ray_gun_fx.pcf",
        include_bytes!("../vanilla/particles/invasion_ray_gun_fx.pcf"),
    ),
    (
        "particles/invasion_unusuals.pcf",
        include_bytes!("../vanilla/particles/invasion_unusuals.pcf"),
    ),
    (
        "particles/halloween2015_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2015_unusuals.pcf"),
    ),
    (
        "particles/rankup.pcf",
        include_bytes!("../vanilla/particles/rankup.pcf"),
    ),
    (
        "particles/halloween2016_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2016_unusuals.pcf"),
    ),
    (
        "particles/rocketpack.pcf",
        include_bytes!("../vanilla/particles/rocketpack.pcf"),
    ),
    (
        "particles/smoke_island_volcano.pcf",
        include_bytes!("../vanilla/particles/smoke_island_volcano.pcf"),
    ),
    (
        "particles/halloween2018_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2018_unusuals.pcf"),
    ),
    (
        "particles/halloween2019_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2019_unusuals.pcf"),
    ),
    (
        "particles/smissmas2019_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2019_unusuals.pcf"),
    ),
    (
        "particles/summer2020_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2020_unusuals.pcf"),
    ),
    (
        "particles/halloween2020_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2020_unusuals.pcf"),
    ),
    (
        "particles/smissmas2020_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2020_unusuals.pcf"),
    ),
    (
        "particles/summer2021_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2021_unusuals.pcf"),
    ),
    (
        "particles/halloween2021_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2021_unusuals.pcf"),
    ),
    (
        "particles/smissmas2021_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2021_unusuals.pcf"),
    ),
    (
        "particles/summer2022_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2022_unusuals.pcf"),
    ),
    (
        "particles/halloween2022_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2022_unusuals.pcf"),
    ),
    (
        "particles/smissmas2022_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2022_unusuals.pcf"),
    ),
    (
        "particles/summer2023_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2023_unusuals.pcf"),
    ),
    (
        "particles/halloween2023_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2023_unusuals.pcf"),
    ),
    (
        "particles/smissmas2023_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2023_unusuals.pcf"),
    ),
    (
        "particles/summer2024_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2024_unusuals.pcf"),
    ),
    (
        "particles/halloween2024_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2024_unusuals.pcf"),
    ),
    (
        "particles/smissmas2024_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2024_unusuals.pcf"),
    ),
    (
        "particles/summer2025_unusuals.pcf",
        include_bytes!("../vanilla/particles/summer2025_unusuals.pcf"),
    ),
    (
        "particles/halloween2025_unusuals.pcf",
        include_bytes!("../vanilla/particles/halloween2025_unusuals.pcf"),
    ),
    (
        "particles/smissmas2025_unusuals.pcf",
        include_bytes!("../vanilla/particles/smissmas2025_unusuals.pcf"),
    ),
];

pub fn graphs() -> ordermap::OrderMap<String, Vec<pcf::Pcf>> {
    use byteorder::{LittleEndian, ReadBytesExt};
    use std::io::Read;
    let graph = include_bytes!(concat!(env!("OUT_DIR"), "/particles.graph"));
    let mut reader = bytes::Buf::reader(&graph[..]);
    let mut result = ordermap::OrderMap::new();
    loop {
        match reader.read_u64::<LittleEndian>() {
            Ok(name_len) => {
                let mut name = vec![0u8; name_len as usize];
                reader.read_exact(&mut name).unwrap();
                let name = String::from_utf8(name).unwrap();

                let pcf_len = reader.read_u64::<LittleEndian>().unwrap();
                assert!(pcf_len > 0);
                let mut pcfs = Vec::with_capacity(pcf_len as usize);
                for _idx in 0..pcf_len {
                    let pcf = pcf::decode(&mut reader).unwrap();
                    pcfs.push(pcf);
                }
                result.insert(name, pcfs);
            }
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            _ => panic!("unexpected EOF"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        os::unix::fs::MetadataExt,
    };

    use pcf::Pcf;

    use crate::particles_manifest::{bins, graphs};

    #[test]
    fn doesnt_panic() {
        let _ = bins();
        let _ = graphs();
    }

    #[test]
    fn equal_to_vanilla_pcfs_with_empty_bodies() {
        let bins = bins();
        for bin in bins {
            let path = format!("vanilla/{}", bin.name());
            let size = fs::metadata(&path).unwrap().size();
            assert_eq!(size, bin.capacity());

            let mut file = File::open_buffered(path).unwrap();
            let pcf = pcf::decode(&mut file).unwrap();

            let empty = Pcf::new_empty_from(&pcf);
            assert_eq!(&empty, bin.as_pcf());
        }
    }
}
