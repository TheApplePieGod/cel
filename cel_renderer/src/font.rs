
use std::path::PathBuf;

use ttf_parser::GlyphId;
use cosmic_text::{fontdb::{self, Query}, Attrs, AttrsList, Buffer, BufferLine, Family, FontSystem, LayoutGlyph, LayoutRun, LayoutRunIter, LineIter, Metrics, ShapeGlyph, ShapeWord, Shaping};

use crate::{glyph_storage::{self, GlyphMetrics, GlyphStorage}, resources::get_resource_path};

#[derive(Clone, Copy)]
pub struct GlyphData {
    pub metrics: GlyphMetrics,
    pub x_pos: f32,
    pub line_idx: u32,
}

pub trait GlyphDataSource<'a> {
    type Run;
    type Glyph;

    fn next_run(&mut self) -> Option<Self::Run>;
    fn glyphs(&self, run: &Self::Run) -> &[Self::Glyph];
    fn convert_glyph(
        &self,
        font_system: &mut FontSystem,
        storage: &mut GlyphStorage,
        glyph: &Self::Glyph
    ) -> GlyphData;
}

pub struct GlyphDataIter<'a, T>
where
    T: GlyphDataSource<'a>,
{
    font_system: &'a mut FontSystem,
    glyph_storage: &'a mut GlyphStorage,
    cur_run: Option<T::Run>,
    cur_glyph: usize,
    source: T,
}

pub struct LayoutGlyphDataSource<'a> {
    iter: LayoutRunIter<'a>,
    cur_line: u32,
}

pub struct GraphemeGlyphDataSource<'a> {
    glyphs: &'a [ShapeGlyph],
    consumed: bool,
}

pub struct ShapeParams {
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub content_width_px: Option<f32>,
    pub content_height_px: Option<f32>,
}

pub struct TextCache {
    buffer: Buffer
}

pub struct Font {
    font_system: FontSystem,
    glyph_storage: GlyphStorage,
    word: ShapeWord,
    aspect_ratio: f32, // Height / width of monospace font
    width_em: f32,
}

impl<'a, T> GlyphDataIter<'a, T>
where
    T: GlyphDataSource<'a>,
{
    pub fn new(
        font_system: &'a mut FontSystem,
        glyph_storage: &'a mut GlyphStorage,
        mut source: T,
    ) -> Self {
        let cur_run = source.next_run();
        Self {
            font_system,
            glyph_storage,
            cur_run,
            cur_glyph: 0,
            source,
        }
    }

}

impl<'a, T> Iterator for GlyphDataIter<'a, T>
where
    T: GlyphDataSource<'a>,
{
    type Item = GlyphData;

    fn next(&mut self) -> Option<GlyphData> {
        while let Some(run) = &self.cur_run {
            let glyphs = self.source.glyphs(run);
            if self.cur_glyph < glyphs.len() {
                let g = &glyphs[self.cur_glyph];
                self.cur_glyph += 1;
                return Some(self.source.convert_glyph(
                    self.font_system,
                    self.glyph_storage,
                    g
                ));
            }
            self.cur_glyph = 0;
            self.cur_run = self.source.next_run();
        }

        None
    }
}

impl<'a> GlyphDataSource<'a> for LayoutGlyphDataSource<'a> {
    type Run = LayoutRun<'a>;
    type Glyph = LayoutGlyph;

    fn next_run(&mut self) -> Option<Self::Run> {
        self.cur_line += 1;
        self.iter.next()
    }

    fn glyphs(&self, run: &Self::Run) -> &[Self::Glyph] {
        &run.glyphs
    }

    fn convert_glyph(
        &self,
        font_system: &mut FontSystem,
        storage: &mut GlyphStorage,
        glyph: &Self::Glyph
    ) -> GlyphData {
        let cache_key = (glyph.glyph_id, glyph.font_id);
        storage.make_glyph_resident(font_system, &cache_key);

        let metrics = *storage.get_glyph_metrics(&cache_key).unwrap();
        GlyphData {
            metrics,
            x_pos: glyph.x,
            line_idx: self.cur_line - 1
        }
    }
}

impl<'a> GlyphDataSource<'a> for GraphemeGlyphDataSource<'a> {
    type Run = ();
    type Glyph = ShapeGlyph;

    fn next_run(&mut self) -> Option<Self::Run> {
        if self.consumed {
            None
        } else {
            self.consumed = true;
            Some(())
        }
    }

    fn glyphs(&self, _: &Self::Run) -> &[Self::Glyph] {
        self.glyphs
    }

    fn convert_glyph(
        &self,
        font_system: &mut FontSystem,
        storage: &mut GlyphStorage,
        glyph: &Self::Glyph
    ) -> GlyphData {
        let cache_key = (glyph.glyph_id, glyph.font_id);
        storage.make_glyph_resident(font_system, &cache_key);

        let metrics = *storage.get_glyph_metrics(&cache_key).unwrap();
        GlyphData {
            metrics,
            x_pos: glyph.x_offset,
            line_idx: 0
        }
    }
}

impl TextCache {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new_empty(Metrics::new(1.0, 1.0))
        }
    }

    pub fn set_text(&mut self, text: &str) {
        let attrs = Attrs::new().family(Family::Monospace);
        let mut max_lines = 0;
        for (i, (range, ending)) in LineIter::new(text).enumerate() {
            let list = AttrsList::new(&attrs);
            if i >= self.buffer.lines.len() {
                self.buffer.lines.push(BufferLine::new(
                    &text[range],
                    ending,
                    list,
                    Shaping::Advanced,
                ));
            } else {
                self.buffer.lines[i].set_text(&text[range], ending, list);
            }
            max_lines += 1;
        }
        if max_lines < self.buffer.lines.len() {
            self.buffer.lines.truncate(max_lines);
        }
    }
}

impl Font {
    pub fn new(primary_font_name: &str, priority_paths: &[PathBuf]) -> Result<Self, String> {
        let mut font_system = FontSystem::new_with_fonts(
            priority_paths.iter().map(|p| fontdb::Source::File(p.clone()))
        );

        font_system.db_mut().set_monospace_family(primary_font_name);

        let mut query: Query = Default::default();
        query.families = &[ Family::Monospace ];
        let (aspect_ratio, width_em) = match font_system.db().query(&query) {
            Some(id) => {
                let font = font_system.get_font(id).unwrap();
                let width = font.rustybuzz().glyph_hor_advance(GlyphId(0)).unwrap_or(0) as f32;
                let height = font.rustybuzz().height() as f32;
                let w_em = width / font.rustybuzz().units_per_em() as f32;
                (height / width, w_em)
            }
            None => (1.75, 1.0)
        };

        let mut glyph_storage = GlyphStorage::new()?;
        glyph_storage.set_target_aspect(aspect_ratio);

        Ok(Self {
            font_system,
            glyph_storage,
            word: ShapeWord { blank: true, glyphs: vec![] },
            aspect_ratio,
            width_em
        })
    }

    pub fn get_primary_name(&self) -> &str { self.font_system.db().family_name(&Family::Monospace) }
    pub fn get_glyph_storage(&self) -> &GlyphStorage { &self.glyph_storage }
    pub fn get_aspect_ratio(&mut self) -> f32 { self.aspect_ratio }
    pub fn get_width_em(&mut self) -> f32 { self.width_em }

    pub fn shape_text<'a>(&'a mut self, params: &ShapeParams, text: &'a mut TextCache)
        -> GlyphDataIter<'a, LayoutGlyphDataSource<'a>>
    {
        let metrics = Metrics::new(params.font_size_px, params.line_height_px);
        text.buffer.set_metrics_and_size(
            &mut self.font_system,
            metrics,
            params.content_width_px,
            params.content_height_px
        );

        // Perform shaping
        text.buffer.shape_until_scroll(&mut self.font_system, false);

        GlyphDataIter::new(
            &mut self.font_system,
            &mut self.glyph_storage,
            LayoutGlyphDataSource {
                iter: text.buffer.layout_runs(),
                cur_line: 0,
            }
        )
    }

    pub fn shape_grapheme<'a>(&'a mut self, text: &str)
        -> GlyphDataIter<'a, GraphemeGlyphDataSource<'a>>
    {
        let attrs = Attrs::new().family(Family::Monospace);

        // Perform shaping
        self.word.build(
            &mut self.font_system,
            text,
            &AttrsList::new(&attrs),
            0..text.len(),
            0.into(),
            false,
            Shaping::Advanced
        );

        GlyphDataIter::new(
            &mut self.font_system,
            &mut self.glyph_storage,
            GraphemeGlyphDataSource {
                glyphs: &self.word.glyphs,
                consumed: false
            }
        )
    }

    pub fn shape_char(&mut self, c: char) -> Option<GlyphData> {
        let mut buf: [u8; 4] = [0; 4];
        let str = c.encode_utf8(&mut buf);

        self.shape_grapheme(str).next()
    }
}
