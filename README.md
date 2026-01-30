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
- Props<sup><small>2</small></sup>

<sup>1. dazzle will install all sound files, but some files may not work.</sup><br />
<sup>2. on linux, requires additional setup. See [below for more info](#linux).</sup>

Dazzle doesn't support these yet, but will soon:

- HUD/VGUI Elements
- Lightwarps
- Decals

## Linux

All features are supported out-of-the-box on Linux, except for certain model customizations - like props.

### I want props!

Prop customization support requires wine and some Source SDK tools. Install wine via the typical means on your linux distro.

To get the necessary Source SDK tools, you have two options:

#### **Option 1.** Install Source SDK Base 2013 Multiplayer w/ Proton. (Recommended)

1. In Steam, open your Library, then find and right-click on "Source SDK Base 2013 Multiplayer". Select "Properties"
2. Click "Compatibility," and enable "Force the use of a specific Steam Play compatibility tool"
3. Select one of the Proton options in the drop down, such as:
    - Proton Hotfix
    - Proton Experimental
    - Proton (any version)
    - GE-Proton (any version)
4. Close out of the Properties window, and Update TF2 if available.
4. Launch Dazzle, and point it to your new Source SDK Base 2013 Multiplayer installation.

#### **Option 2.** Force TF2 to use Proton. **NOT RECOMMENDED**

> [!WARNING]
> This option is only recommended if _you know what you are doing_, or if you already use Proton with TF2. If you play TF2 natively on Linux, switching to Proton will fundamentally alter various aspects of the game and may be outright incompatible with your hardware.

1. In Steam, open your Library and right-click on Team Fortress 2. Select "Properties"
2. Click "Compatibility," and enable "Force the use of a specific Steam Play compatibility tool"
3. Select one of the Proton options in the drop down, such as:
    - Proton Hotfix
    - Proton Experimental
    - Proton (any version)
    - GE-Proton (any version)
4. Close out of the Properties window, and Update TF2 if available.
5. Launch Dazzle - it will automatically detect StudioMDL from your TF2 installation.

### Why do I need to do this?

Dazzle uses a model precaching technique that ensures certain model customizations - like props - are always chosen over the vanilla versions of those models. This technique requires StudioMDL - a proprietary program that is always distributed with TF2 **on windows**, but has no known Linux distribution - at least none that don't violate the Steam Subscriber Agreement.

Thankfully, you can install the windows versions of Source SDK Base 2013 Multiplayer or TF2 by enabling Proton. It's far from ideal, but it works!

### Why don't I need to do this with cueki's preloader?

Unfourtunately, cueki is violating the [Steam Subscriber Agreement](https://store.steampowered.com/subscriber_agreement/) by distributing StudioMDL herself, under an [incorrect license](https://github.com/cueki/studiomdl/blob/main/LICENSE) - this is actually the "SOURCE 1 SDK LICENSE" found in [source-sdk-2013](https://github.com/ValveSoftware/source-sdk-2013/blob/master/LICENSE). StudioMDL is a proprietary tool that [is not distributed in source-sdk-2013](https://github.com/search?q=repo%3AValveSoftware%2Fsource-sdk-2013+path%3A*studiomdl*&type=code), and has never been distributed under any license other than the Steam Subscriber Agreement - except in some special circumstances (such as 3rd party partners who have access to the Source SDK's source code).

While I'm no copyright bootlicker, I must abide by these licenses in order to develop dazzle in good faith.

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
