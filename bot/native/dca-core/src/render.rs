//! SVG -> PNG with resvg. Fonts come from the bundled Noto files plus whatever the system has (CJK, Arabic, ...),
//! so player names in any script show up in the generated spreadsheet images and report tables.

use resvg::{tiny_skia, usvg};
use std::path::Path;
use std::sync::Arc;

pub struct Renderer {
    options: usvg::Options<'static>,
}

impl Renderer {
    pub fn new(fonts_dir: &Path) -> Renderer {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        if fonts_dir.is_dir() {
            db.load_fonts_dir(fonts_dir);
        }
        let sans = ["Noto Sans", "DejaVu Sans", "Liberation Sans", "Arial"];
        let serif = ["Noto Serif", "DejaVu Serif", "Liberation Serif"];
        let pick = |candidates: &[&str], db: &usvg::fontdb::Database| -> String {
            candidates
                .iter()
                .find(|c| db.faces().any(|f| f.families.iter().any(|(name, _)| name == *c)))
                .map(|c| c.to_string())
                .unwrap_or_else(|| candidates[0].to_string())
        };
        let sans_family = pick(&sans, &db);
        let serif_family = pick(&serif, &db);
        db.set_sans_serif_family(sans_family.clone());
        db.set_serif_family(serif_family.clone());
        db.set_cursive_family(sans_family.clone());
        db.set_fantasy_family(sans_family.clone());
        db.set_monospace_family(sans_family.clone());

        let mut options = usvg::Options::default();
        options.fontdb = Arc::new(db);
        options.font_family = sans_family;
        Renderer { options }
    }

    /// Render an SVG document to PNG bytes.
    pub fn svg_to_png(&self, svg: &str) -> Result<Vec<u8>, String> {
        // The generated SVGs ask for Arial/Georgia; map them to the families that exist on this machine.
        let sans = &self.options.font_family;
        let svg = svg.replace("font-family=\"Arial\"", &format!("font-family=\"{sans}\""));
        let tree = usvg::Tree::from_str(&svg, &self.options).map_err(|e| format!("invalid SVG: {e}"))?;
        let size = tree.size().to_int_size();
        let mut pixmap = tiny_skia::Pixmap::new(size.width().max(1), size.height().max(1)).ok_or("image too large to render")?;
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
        pixmap.encode_png().map_err(|e| format!("PNG encoding failed: {e}"))
    }
}
