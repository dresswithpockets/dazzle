use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui;
use eframe::egui::FontFamily;
use eframe::egui::FontId;
use eframe::egui::TextStyle;
use eframe::egui::FontData;
use eframe::egui::FontDefinitions;

pub(crate) fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "comic_neue_regular".to_string(),
        Arc::new(FontData::from_static(include_bytes!("static/fonts/Comic_Neue/ComicNeue-Regular.ttf"))),
    );

    fonts.font_data.insert(
        "robot_variant_wght".to_string(),
        Arc::new(FontData::from_static(include_bytes!("static/fonts/Roboto_Mono/RobotoMono-VariableFont_wght.ttf"))),
    );

    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "comic_neue_regular".to_owned());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "robot_variant_wght".to_owned());

    ctx.set_fonts(fonts);
}

#[inline]
pub(crate) fn big() -> TextStyle {
    TextStyle::Name("Big".into())
}

pub(crate) fn configure_text_styles(ctx: &egui::Context) {
    use FontFamily::{Monospace, Proportional};

    let text_styles: BTreeMap<TextStyle, FontId> = [
        (TextStyle::Heading, FontId::new(25.0, Proportional)),
        (big(), FontId::new(16.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, Monospace)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
    ].into();

    ctx.all_styles_mut(move |style| style.text_styles = text_styles.clone());
}
