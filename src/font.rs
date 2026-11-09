use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fontdb::{Database, Family, ID, Query, Source, Stretch, Style, Weight};

use crate::error::{RenderError, RenderResult};

const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;

fn push_unique(target: &mut Vec<ID>, id: ID) {
    if !target.contains(&id) {
        target.push(id);
    }
}

fn load_faces(db: &mut Database, data: Vec<u8>, label: &str) -> RenderResult<Vec<ID>> {
    if data.is_empty() {
        return Err(RenderError::font(format!("{label} font data is empty")));
    }
    if data.len() > MAX_FONT_BYTES {
        return Err(RenderError::font(format!(
            "{label} font is {} bytes, max is {MAX_FONT_BYTES}",
            data.len()
        )));
    }

    let source: Source = Source::Binary(Arc::new(data) as Arc<dyn AsRef<[u8]> + Send + Sync>);
    let ids: Vec<ID> = db.load_font_source(source).into_iter().collect();
    if ids.is_empty() {
        return Err(RenderError::font(format!(
            "{label} does not contain a usable font face"
        )));
    }
    Ok(ids)
}

fn query_system_families(db: &Database, weight: Weight) -> Vec<ID> {
    const FAMILIES: &[Family<'_>] = &[
        Family::SansSerif,
        Family::Serif,
        Family::Monospace,
        Family::Name("Noto Sans"),
        Family::Name("Noto Sans CJK SC"),
        Family::Name("Noto Sans CJK JP"),
        Family::Name("Noto Sans CJK"),
        Family::Name("Noto Serif CJK SC"),
        Family::Name("WenQuanYi Micro Hei"),
        Family::Name("Droid Sans Fallback"),
        Family::Name("Noto Sans Arabic"),
        Family::Name("Noto Sans Arabic UI"),
        Family::Name("Noto Naskh Arabic"),
        Family::Name("Noto Kufi Arabic"),
        Family::Name("Noto Sans Hebrew"),
        Family::Name("Noto Sans Thai"),
        Family::Name("Noto Sans Devanagari"),
        Family::Name("Noto Sans Bengali"),
        Family::Name("Noto Sans Tamil"),
        Family::Name("Noto Sans Telugu"),
        Family::Name("Noto Sans Malayalam"),
        Family::Name("Noto Sans Gujarati"),
        Family::Name("Noto Sans Gurmukhi"),
        Family::Name("Noto Sans Kannada"),
        Family::Name("Noto Sans Sinhala"),
        Family::Name("Noto Sans Myanmar"),
        Family::Name("Noto Sans Khmer"),
        Family::Name("Noto Sans Lao"),
        Family::Name("Noto Sans Georgian"),
        Family::Name("Noto Sans Armenian"),
        Family::Name("DejaVu Sans"),
        Family::Name("Liberation Sans"),
        Family::Name("Noto Emoji"),
        Family::Name("Noto Sans Symbols"),
        Family::Name("Noto Sans Symbols 2"),
    ];

    let mut ids = Vec::new();
    for family in FAMILIES {
        let query = Query {
            families: std::slice::from_ref(family),
            weight,
            stretch: Stretch::Normal,
            style: Style::Normal,
        };
        if let Some(id) = db.query(&query) {
            push_unique(&mut ids, id);
        }
    }
    ids
}

pub struct FontStore {
    db: Database,
    regular_chain: Vec<ID>,
    bold_chain: Vec<ID>,
    /// char code -> index in the selected chain
    char_cache: Mutex<HashMap<(u32, bool), usize>>,
}

impl FontStore {
    pub fn new(
        regular: Vec<u8>,
        bold: Option<Vec<u8>>,
        fallback: Vec<Vec<u8>>,
        system_fonts: bool,
    ) -> RenderResult<Self> {
        let mut db = Database::new();

        let regular_ids = load_faces(&mut db, regular, "regular")?;
        let bold_ids = match bold {
            Some(data) => load_faces(&mut db, data, "bold")?,
            None => Vec::new(),
        };

        let mut fallback_ids = Vec::new();
        for (index, data) in fallback.into_iter().enumerate() {
            let ids = load_faces(&mut db, data, &format!("fallback[{index}]"))?;
            for id in ids {
                push_unique(&mut fallback_ids, id);
            }
        }

        let (system_regular, system_bold) = if system_fonts {
            db.load_system_fonts();
            (
                query_system_families(&db, Weight::NORMAL),
                query_system_families(&db, Weight::BOLD),
            )
        } else {
            (Vec::new(), Vec::new())
        };

        let mut regular_chain = Vec::new();
        for id in &regular_ids {
            push_unique(&mut regular_chain, *id);
        }
        for id in &bold_ids {
            push_unique(&mut regular_chain, *id);
        }
        for id in &fallback_ids {
            push_unique(&mut regular_chain, *id);
        }
        for id in &system_regular {
            push_unique(&mut regular_chain, *id);
        }

        let mut bold_chain = Vec::new();
        for id in &bold_ids {
            push_unique(&mut bold_chain, *id);
        }
        for id in &regular_ids {
            push_unique(&mut bold_chain, *id);
        }
        for id in &fallback_ids {
            push_unique(&mut bold_chain, *id);
        }
        for id in &system_bold {
            push_unique(&mut bold_chain, *id);
        }
        for id in &system_regular {
            push_unique(&mut bold_chain, *id);
        }

        Ok(Self {
            db,
            regular_chain,
            bold_chain,
            char_cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn chain(&self, bold: bool) -> &[ID] {
        if bold {
            &self.bold_chain
        } else {
            &self.regular_chain
        }
    }

    pub fn face_id(&self, chain_index: usize, bold: bool) -> ID {
        self.chain(bold)[chain_index]
    }

    pub fn face_for_char(&self, ch: char, bold: bool) -> usize {
        {
            let cache = self
                .char_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = cache.get(&(ch as u32, bold)) {
                return *index;
            }
        }

        let chain = self.chain(bold);
        let mut found = 0;
        for (index, id) in chain.iter().enumerate() {
            let has_glyph = self
                .db
                .with_face_data(*id, |data, face_index| {
                    ttf_parser::Face::parse(data, face_index)
                        .ok()
                        .and_then(|face| face.glyph_index(ch))
                        .is_some()
                })
                .unwrap_or(false);
            if has_glyph {
                found = index;
                break;
            }
        }

        let mut cache = self
            .char_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert((ch as u32, bold), found);
        found
    }
}
