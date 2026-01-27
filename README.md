# Dazzle

**dazzle** is a mod installer for Team Fortress 2.

Dazzle will automatically process & install the following kinds of mods:

- Particles
- Player Models, Animations, Face Flexes
- Weapon Models & Animations
- Sounds*

<small>\* dazzle will install all sound files, but some may not work.</small>

Dazzle doesn't support these yet, but will soon:

- Props
- HUD/VGUI Elements
- Skyboxes
- Lightwarps
- Warpaints
- Decals

## Credits

Some of the techniques dazzle uses were inspired by [cueki](https://github.com/cueki/)'s [preloader](https://github.com/cueki/casual-pre-loader/)

Dazzle's prop model precache is based on [GoopSwagger](https://github.com/Lucifixion)'s [QuickPrecache](https://github.com/Lucifixion/QuickPrecache)

## Build from source

Dazzle can be built with Rust v1.94 Nightly.

Dazzle has a compile-time dependency on the the vanilla TF2 particles & particles manifest. After cloning, you must extract `particles/` from `tf/tf2_misc_dir.vpk` into `dazzle/dazzle/vanilla/particles/`.

You can extract files from VPKs with [VPKEdit](https://developer.valvesoftware.com/wiki/VPKEdit).

Then you can just build & run:

```sh
cargo build --release
./target/release/dazzle
```
