# Dazzle

**dazzle** is a mod installer for Team Fortress 2.

Dazzle will automatically process & install the following kinds of mods:

- Particles
- Player Models, Animations, Face Flexes
- Weapon Models, Animations
- Sounds<sup><small>1</small></sup>
- Skyboxes
- Warpaints
- HUDs/VGUI
- Configs

<sup>1. dazzle will install all sound files, but some files may not work.</sup><br />

Dazzle doesn't support these yet, but will soon:

- Props
- Lightwarps
- Decals

## Credits

Some of the techniques dazzle uses were inspired by [cueki](https://github.com/cueki/)'s [preloader](https://github.com/cueki/casual-pre-loader/)

Dazzle's prop model precache is based on [GoopSwagger](https://github.com/Lucifixion)'s [QuickPrecache](https://github.com/Lucifixion/QuickPrecache)

## Build from source

Dazzle can be built with Rust v1.95 Nightly.

Dazzle has a compile-time dependency on the the vanilla TF2 particles & particles manifest. After cloning, you must extract `particles/` from `tf/tf2_misc_dir.vpk` into `dazzle/dazzle/vanilla/particles/`.

You can extract files from VPKs with [VPKEdit](https://developer.valvesoftware.com/wiki/VPKEdit).

Then you can just build & run:

```sh
cargo build --release
./target/release/dazzle
```
