use cosmic_text::FontSystem;
use cosmic_text::fontdb::{Database, Source};
use std::sync::Arc;

const UI_FAMILY: &str = "Inter";

static BUNDLED: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-BoldItalic.ttf"),
];

pub fn bundled() -> FontSystem {
    let mut db = Database::new();
    for font in BUNDLED {
        db.load_font_source(Source::Binary(Arc::new(font)));
    }
    db.set_sans_serif_family(UI_FAMILY);
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
    FontSystem::new_with_locale_and_db(locale, db)
}

pub fn system() -> Database {
    let mut db = Database::new();
    db.load_system_fonts();
    db
}

pub fn add_system(font_system: &mut FontSystem, system: Database) {
    let db = font_system.db_mut();
    for face in system.faces() {
        if face.families.iter().all(|(family, _)| family != UI_FAMILY) {
            db.push_face_info(face.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::fontdb::{Family, Query, Stretch, Style, Weight};

    #[test]
    fn every_ui_style_is_bundled() {
        let fonts = bundled();
        for weight in [Weight::NORMAL, Weight::BOLD] {
            for style in [Style::Normal, Style::Italic] {
                let query = Query {
                    families: &[Family::SansSerif],
                    weight,
                    stretch: Stretch::Normal,
                    style,
                };
                let id = fonts.db().query(&query).expect("no bundled face");
                let face = fonts.db().face(id).unwrap();
                assert_eq!(face.families[0].0, UI_FAMILY);
                assert_eq!((face.weight, face.style), (weight, style));
            }
        }
    }

    #[test]
    fn system_fonts_never_replace_the_bundled_inter() {
        let mut fonts = bundled();
        let mut system = Database::new();
        for font in BUNDLED {
            system.load_font_source(Source::Binary(Arc::new(font)));
        }
        add_system(&mut fonts, system);
        assert_eq!(fonts.db().len(), BUNDLED.len());
    }
}
