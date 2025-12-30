//! TF2 asset preloader based on and compatible with cueki's casual preloader.
//!
//! It supports these mods:
//!
//! - Particles
//! - Models
//! - Animations
//! - VGUI elements
//! - Lightwarps
//! - Skyboxes
//! - Warpaints
//! - Game sounds
//!
//! # Why?
//!
//! Cueki has done a good amount of work creating a usable preloader. The goal is to create a simpler and more 
//! performant implementation.
//!
//! I'm also using this as a means to practice more idiomatic Rust.

#![feature(assert_matches)]
#![feature(duration_constructors)]
#![warn(clippy::pedantic)]

fn main() {
    /*
       TODO: on first-run establish an application folder for configuration & storing unprocessed mods
       TODO: if not already configured, detect/select a tf/ directory
       TODO: tui for configuring enabled/disabled custom particles found in addons
       TODO: tui for selecting addons to install/uninstall
       TODO: detect conflicts in selected addons
       TODO: process addons and pack into a custom VPK
    */

    /*
     technical work:
       TODO: port PCK parser
       TODO: port VPK parser

       General technical process:
           - more...
           - patches tf_misc_dir.vpk with particles
           - patches hud overrides
           - generates VMTs
           - creates a _QuickPrecache.vpk for precached map props
           - generates a w/config.cfg for execution at launch (preloading, etc)
           - packs processed mods into custom vpk
    */

    // starting out, we're going to get custom particles working
}
