use rust_fontconfig::{FcFontCache, FcPattern};
use ttf_parser::{Face, GlyphId, RasterGlyphImage};
use msdfgen::{FontExt, Bitmap, Range, MsdfGeneratorConfig, FillRule, Bound, Shape};
use std::{fs::File, io::{BufReader, Read}, collections::{HashMap, hash_map::Entry}};
use std::num::NonZeroUsize;
use lru::LruCache;
use crate::texture::Texture;

pub type FontCache = FcFontCache;

const ATLAS_SIZE: u32 = 1024;
const MSDF_SIZE: u32 = 32;
const MSDF_RANGE: f32 = 3.0;

// Normalized to width=1
#[derive(Clone, Copy, Default)]
pub struct FaceMetrics {
    pub height: f32,
    pub width: f32,
    pub descender: f32,
    // Scale when working directly with the font glyphs
    pixel_scale: f64,
    scale_x: f64,
    scale_y: f64
}

#[derive(Clone, Copy, Debug)]
pub enum RenderType {
    MSDF,
    RASTER
}

#[derive(Clone, Copy)]
pub struct GlyphMetrics {
    pub atlas_index: u32,
    pub atlas_uv: Bound<f32>,
    pub glyph_bound: Bound<f32>,
    pub atlas_bound: Bound<f64>,
    pub render_type: RenderType
}

pub struct GlyphData {
    pixels: Vec<f32>,
    metrics: GlyphMetrics
}

#[derive(Clone, Copy, Default)]
struct GlyphBox {
    range: f64,
    scale: f64,
    rect: msdfgen::Vector2<f64>,
    translate: msdfgen::Vector2<f64>
}

struct FontData {
    name: String,
    bytes: Vec<u8>,
    metrics: FaceMetrics
}

// If not None, indicates font index. Then, the first argument is glyph index.
// Otherwise, first argument is generic char
type CacheKey = (u32, Option<usize>);

pub struct Font {
    font_data: Vec<FontData>,
    glyph_cache: HashMap<CacheKey, GlyphData>,
    glyph_lru: LruCache<CacheKey, GlyphMetrics>,
    atlas_free_list: u32,
    atlas_tex: Texture<f32>,
    font_cache: FontCache
}

impl Default for GlyphMetrics {
    fn default() -> Self {
        Self {
            atlas_index: 0,
            atlas_uv: Default::default(),
            glyph_bound: Bound::new(0.0, 0.0, 1.0, 1.0),
            atlas_bound: Default::default(),
            render_type: RenderType::RASTER
        }
    }
}

impl Default for GlyphData {
    fn default() -> Self {
        Self {
            pixels: vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize],
            metrics: Default::default()
        }
    }
}

impl Font {
    pub fn new(
        name_list: &Vec<&str>,
    ) -> Result<Self, String> {
        let font_cache = FontCache::build();

        /*
        for font in font_cache.list() {
            log::warn!("Found {}", &font.0.name.as_ref().unwrap_or(&String::new()));
        }
        */

        let mut font_data = vec![];
        for name in name_list {
            let mut loaded = Self::load_font_from_resources(&name);
            if loaded.is_err() {
                // Fallback to name query
                loaded = Self::load_font_by_name(&font_cache, &name);
            }

            match loaded {
                Ok(data) => {
                    let face = Face::parse(data.as_slice(), 0).unwrap();
                    font_data.push(FontData {
                        name: name.to_string(),
                        metrics: Font::parse_face_metrics(&face),
                        bytes: data,

                    })
                },
                Err(msg) => log::warn!("Font '{}' failed to load: {}", name, msg)
            }
        }

        if font_data.is_empty() {
            return Err("All fonts failed to load".to_string());
        }

        // Generate font atlas data
        let max_glyphs = (ATLAS_SIZE / MSDF_SIZE) * (ATLAS_SIZE / MSDF_SIZE);
        let mut atlas_tex = Texture::new(ATLAS_SIZE, ATLAS_SIZE, 4, true, None)?;

        // Populate index zero
        atlas_tex.update_pixels(
            0, 0,
            MSDF_SIZE, MSDF_SIZE,
            &vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize]
        );

        Ok(Self {
            font_data,
            glyph_cache: Default::default(),
            glyph_lru: LruCache::new(NonZeroUsize::new(max_glyphs as usize).unwrap()),
            atlas_free_list: 1, // Spot zero is always empty
            atlas_tex,
            font_cache
        })
    }

    pub fn get_primary_name(&self) -> &str {
        &self.font_data[0].name
    }

    pub fn get_primary_metrics(&self) -> &FaceMetrics {
        &self.font_data[0].metrics
    }

    pub fn get_atlas_tex(&self) -> &Texture<f32> {
        &self.atlas_tex
    }

    pub fn get_atlas_size(&self) -> u32 {
        ATLAS_SIZE
    }

    pub fn get_glyph_size(&self) -> u32 {
        MSDF_SIZE
    }

    pub fn get_pixel_range(&self) -> f32 {
        MSDF_RANGE
    }

    pub fn get_atlas_texcoord(atlas_index: u32) -> [f64; 2] {
        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        return Self::convert_atlas_offset_to_texcoord(offset);
    }

    pub fn get_glyph_data(&mut self, key: char) -> GlyphMetrics {
        self.get_glyph_data_internal(key as u32, None)
    }

    pub fn get_grapheme_data(&mut self, key: &str) -> Vec<GlyphMetrics> {
        // TODO: cache
        let mut output = vec![];
        for (font_idx, glyphs) in self.parse_grapheme(key).iter() {
            for (info, _pos) in glyphs.iter() {
                //log::warn!("{} {} {}", key, _pos.x_advance, _pos.y_advance);
                output.push(self.get_glyph_data_internal(info.codepoint, Some(*font_idx)));
            }
        }
        output
    }

    fn get_glyph_data_internal(&mut self, key: u32, font_index: Option<usize>) -> GlyphMetrics {
        // TODO: don't need to evict if new glyph is invalid (default to index 0)
        match self.glyph_lru.get(&(key, font_index)) {
            Some(v) => *v,
            None => {
                let atlas_index: u32;
                if self.atlas_free_list < self.glyph_lru.cap().get() as u32 {
                    // Pull from free list first
                    atlas_index = self.atlas_free_list;
                    self.atlas_free_list += 1;
                } else {
                    // LRU Will always be full here since the LRU is the same
                    // size as the free list, so we can safely evict
                    let lru = self.glyph_lru.pop_lru().unwrap();
                    atlas_index = lru.1.atlas_index;
                }

                let glyph_data = self.update_char_in_atlas(key, atlas_index, font_index);

                glyph_data.metrics
            }
        }
    }

    fn update_char_in_atlas(&mut self, key: u32, atlas_index: u32, font_index: Option<usize>) -> &GlyphData {
        // TODO: batching for face parsing 
        let cache_key = (key, font_index);
        if !self.glyph_cache.contains_key(&cache_key) {
            let new_data = match font_index {
                Some(font_index) => self.load_glyph_from_index(key, font_index),
                None => self.load_glyph_from_char(char::from_u32(key).unwrap())
            };
            self.glyph_cache.insert(cache_key, new_data);
        }

        let glyph_data = self.glyph_cache.get_mut(&cache_key).unwrap();

        // Update atlas pixels
        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        self.atlas_tex.update_pixels(
            offset[0], offset[1],
            MSDF_SIZE, MSDF_SIZE,
            &glyph_data.pixels
        );

        // Update the atlas position of the glyph & compute new UV
        // UV.y is flipped since the underlying atlas bitmaps have flipped y
        glyph_data.metrics.atlas_index = atlas_index;
        let uv = Font::get_atlas_texcoord(atlas_index);
        let uv_min = [
            uv[0] + glyph_data.metrics.atlas_bound.left,
            uv[1] + glyph_data.metrics.atlas_bound.top,
        ];
        let uv_max = [
            uv_min[0] + glyph_data.metrics.atlas_bound.width(),
            uv_min[1] - glyph_data.metrics.atlas_bound.height(),
        ];
        glyph_data.metrics.atlas_uv = Bound::new(
            uv_min[0] as f32,
            uv_max[1] as f32,
            uv_max[0] as f32,
            uv_min[1] as f32,
        );

        self.glyph_lru.put(cache_key, glyph_data.metrics);

        glyph_data
    }

    fn parse_grapheme(&self, grapheme: &str) -> Vec<(usize, Vec<(harfbuzz_rs::GlyphInfo, harfbuzz_rs::GlyphPosition)>)> {
        // TODO: optimize 

        // TODO: reuse
        let mut supported_chars = String::new();
        let mut buf = harfbuzz_rs::UnicodeBuffer::new();

        let mut result = Vec::new();
        let mut chars = grapheme.chars().peekable();
        
        while chars.peek().is_some() {
            for (font_index, font_data) in self.font_data.iter().enumerate() {
                let ttf_face = ttf_parser::Face::parse(&font_data.bytes, 0).unwrap();

                // Find the longest substring supported by this font
                supported_chars.clear();
                let mut iter = chars.clone();
                while let Some(&c) = iter.peek() {
                    if ttf_face.glyph_index(c).is_some() {
                        supported_chars.push(c);
                        iter.next();
                    } else {
                        break;
                    }
                }

                if supported_chars.is_empty() {
                    continue;
                }

                // Shape using HarfBuzz
                buf = buf.add_str(&supported_chars);
                let hb_face = harfbuzz_rs::Face::from_bytes(&font_data.bytes, 0);
                let hb_font = harfbuzz_rs::Font::new(hb_face);
                let hb_shape = harfbuzz_rs::shape(&hb_font, buf, &[]);
                
                let glyphs = hb_shape
                    .get_glyph_infos()
                    .iter()
                    .zip(hb_shape.get_glyph_positions())
                    .map(|(info, pos)| (info.clone(), pos.clone()))
                    .collect();
                
                result.push((font_index, glyphs));

                // Advance chars iterator to the next unsupported character
                // TODO: optimize
                for _ in 0..supported_chars.chars().count() {
                    chars.next();
                }

                buf = hb_shape.clear();

                break;
            }

            if supported_chars.is_empty() {
                let unsupported_char = chars.next().unwrap();
                log::warn!("Missing font support for {:?}", unsupported_char);
            }
        }

        result
    }

    fn load_glyph_from_index(&self, index: u32, font_index: usize) -> GlyphData {
        let face = Face::parse(&self.font_data[font_index].bytes, 0).unwrap();
        self.load_glyph(
            &self.font_data[font_index].metrics,
            &face,
            GlyphId(index as u16)
        )
    }

    fn load_glyph_from_char(&self, key: char) -> GlyphData {
        let mut face: Face;
        let glyph_id: GlyphId;

        // Search for the glyph in loaded fonts
        let mut index = 0;
        loop {
            if index >= self.font_data.len() {
                log::warn!("Missing glyph for '{:?}'", key);
                return Default::default();
            }

            face = Face::parse(&self.font_data[index].bytes, 0).unwrap();
            if let Some(id) = face.glyph_index(key) {
                glyph_id = id;
                break;
            }

            index += 1;
        }

        self.load_glyph(&self.font_data[index].metrics, &face, glyph_id)
    }

    fn load_glyph(&self, metrics: &FaceMetrics, face: &Face, id: GlyphId) -> GlyphData {
        let render_type: RenderType;
        let raster: Option<RasterGlyphImage>;
        let mut shape: Option<Shape> = None;

        raster = face.glyph_raster_image(id, MSDF_SIZE as u16);
        if raster.is_some() {
            render_type = RenderType::RASTER;
        } else {
            shape = face.glyph_shape(id);
            if shape.is_some() {
                render_type = RenderType::MSDF;
            } else {
                log::warn!("Missing data for glyph id {}", id.0);
                return Default::default();
            }
        }

        let (pixels, glyph_bound, pixel_bound) = match render_type {
            RenderType::MSDF => self.generate_msdf(metrics, shape.unwrap()),
            RenderType::RASTER => self.generate_raster(raster.unwrap())
        };

        // For debugging
        /*
        let bytes = pixels.iter().map(|p| (p * 255.0) as u8).collect::<Vec<u8>>();
        let _ = image::save_buffer(
            format!("glyphs/{}.png", id.0),
            &bytes, MSDF_SIZE, MSDF_SIZE, image::ColorType::Rgba8
        );
        let glyph_name = face.glyph_name(id).unwrap_or("unknown");
        println!("Pixel bound for {}: T:{} B:{} L:{} R:{}", glyph_name, pixel_bound.top, pixel_bound.bottom, pixel_bound.left, pixel_bound.right);
        println!("Glyph bound for {}: T:{} B:{} L:{} R:{}", glyph_name, glyph_bound.top, glyph_bound.bottom, glyph_bound.left, glyph_bound.right);
        */

        // Compute final atlas bound (uv coords)
        let texcoord_scale = 1.0 / self.get_atlas_size() as f64;
        let atlas_bound = Bound::new(
            pixel_bound.left * texcoord_scale,
            pixel_bound.bottom * texcoord_scale,
            pixel_bound.right * texcoord_scale,
            pixel_bound.top * texcoord_scale
        );

        GlyphData {
            pixels,
            metrics: GlyphMetrics {
                atlas_index: 0,
                atlas_uv: Bound::new(0.0, 0.0, 0.0, 0.0),
                glyph_bound,
                atlas_bound,
                render_type
            }
        }
    }

    fn get_msdf_box(scale: f64, shape: &mut Shape) -> GlyphBox {
        // Loosely based on 
        // https://github.com/Chlumsky/msdf-atlas-gen/blob/master/msdf-atlas-gen/GlyphGeometry.cpp

        // Helper to compute boundaries and dimensions given a scale
        let compute_boundaries = |scale: f64, bounds: &msdfgen::Bound<f64>| -> (f64, f64, f64, f64, f64, f64) {
            let sl = (scale * bounds.left - 0.5).floor();
            let sr = (scale * bounds.right + 0.5).ceil();
            let sb = (scale * bounds.bottom - 0.5).floor();
            let st = (scale * bounds.top + 0.5).ceil();
            let width = sr - sl;
            let height = st - sb;
            (sl, sr, sb, st, width, height)
        };

        let mut bbox = GlyphBox::default();
        let mut bounds = shape.get_bound();
        let mut scale = MSDF_SIZE as f64 * scale;
        let range = MSDF_RANGE as f64 / scale;
        let miter_limit = 1.0;

        bbox.scale = scale;
        bbox.range = range;

        if bounds.left < bounds.right && bounds.bottom < bounds.top {
            if miter_limit > 0.0 {
                shape.bound_miters(&mut bounds, -range, miter_limit, msdfgen::Polarity::Positive);
            }

            // Compute provisional pixel boundaries
            let (mut sl, _, mut sb, _, mut width, mut height) = compute_boundaries(scale, &bounds);

            // Check if the glyph exceeds the maximum allowed size
            let scale_adjust = if width > MSDF_SIZE as f64 || height > MSDF_SIZE as f64 {
                let factor_width  = if width  > MSDF_SIZE as f64 { MSDF_SIZE as f64 / width } else { 1.0 };
                let factor_height = if height > MSDF_SIZE as f64 { MSDF_SIZE as f64 / height } else { 1.0 };
                factor_width.min(factor_height)
            } else {
                1.0
            };

            // If the glyph is too large, adjust the scale and recalc boundaries
            if scale_adjust < 1.0 {
                scale *= scale_adjust;
                bbox.scale = scale;
                bbox.range = MSDF_RANGE as f64 / scale;
                (sl, _, sb, _, width, height) = compute_boundaries(scale, &bounds);
            }
            
            bbox.translate.x = -sl / scale;
            bbox.translate.y = -sb / scale;
            bbox.rect.x = width;
            bbox.rect.y = height;
        } else {
            // Invalid bounds
            bbox.rect = msdfgen::Vector2::default();
            bbox.translate = msdfgen::Vector2::default();
        }

        bbox
    }

    fn generate_msdf(&self, metrics: &FaceMetrics, shape: Shape) -> (Vec<f32>, Bound<f32>, Bound<f64>) {
        let mut shape = shape;
        shape.normalize();

        let bbox = Font::get_msdf_box(metrics.pixel_scale, &mut shape);
        let framing = msdfgen::Framing {
            projection: msdfgen::Projection {
                translate: bbox.translate,
                scale: bbox.scale.into()
            },
            range: bbox.range
        };

        let config = MsdfGeneratorConfig::default();
        let fill_rule = FillRule::default();
        let mut bitmap = Bitmap::new(MSDF_SIZE, MSDF_SIZE);
        shape.edge_coloring_simple(3.0, 0);
        shape.generate_mtsdf(&mut bitmap, &framing, &config);

        shape.correct_sign(&mut bitmap, &framing, fill_rule);
        shape.correct_msdf_error(&mut bitmap, &framing, &config);
        
        let bmp_pixels = bitmap.pixels();
        let mut pixels: Vec<f32> = vec![0.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize];
        for i in 0..bmp_pixels.len() {
            pixels[i * 4 + 0] = bmp_pixels[i].r;
            pixels[i * 4 + 1] = bmp_pixels[i].g;
            pixels[i * 4 + 2] = bmp_pixels[i].b;
            pixels[i * 4 + 3] = bmp_pixels[i].a;
        }

        // https://github.com/Chlumsky/msdf-atlas-gen/blob/master/msdf-atlas-gen/GlyphGeometry.cpp
        // Ensure we scale by the font's scale so that width and height are normalized
        // to 1.0
        let mut glyph_bound = Bound::new(
            (metrics.scale_x * (-bbox.translate.x + 0.5 / bbox.scale)) as f32,
            (metrics.scale_y * (-bbox.translate.y + 0.5 / bbox.scale)) as f32,
            (metrics.scale_x * (-bbox.translate.x + (bbox.rect.x - 0.5) / bbox.scale)) as f32,
            (metrics.scale_y * (-bbox.translate.y + (bbox.rect.y - 0.5) / bbox.scale)) as f32,
        );

        // Bump t/b glyph bound to ensure baseline is normalized to 0
        glyph_bound.bottom += metrics.descender;
        glyph_bound.top += metrics.descender;

        let mut pixel_bound = Bound::new(
            0.5,
            0.5,
            bbox.rect.x - 0.5,
            bbox.rect.y - 0.5,
        );

        // Clamp glyph bound and adjust pixel bound accordingly 
        if glyph_bound.bottom < 0.0 {
            let adjust = glyph_bound.bottom.abs();
            glyph_bound.bottom = 0.0;
            pixel_bound.bottom += bbox.rect.y * adjust as f64;
        }
        if glyph_bound.top > 1.0 {
            let adjust = glyph_bound.top - 1.0;
            glyph_bound.top = 1.0;
            pixel_bound.top -= bbox.rect.y * adjust as f64;
        }

        (pixels, glyph_bound, pixel_bound)
    }

    fn generate_raster(&self, raster: RasterGlyphImage) -> (Vec<f32>, Bound<f32>, Bound<f64>) {
        let glyph_bound = Bound::new(0.0, 0.05, 1.0, 0.95);
        let pixel_bound = Bound::new(0.5, 0.5, MSDF_SIZE as f64 - 0.5, MSDF_SIZE as f64 - 0.5);

        let mut image = match image::load_from_memory(raster.data) {
            Ok(img) => img,
            Err(err) => {
                log::warn!("Glyph had invalid raster image: {}", err.to_string());
                let pixels: Vec<f32> = vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize];
                return (pixels, Default::default(), Default::default());
            }
        };

        image = image.flipv().resize(
            MSDF_SIZE,
            MSDF_SIZE,
            image::imageops::FilterType::Lanczos3
        );

        let pixels = image.to_rgba32f().to_vec();

        (pixels, glyph_bound, pixel_bound)
    }

    fn parse_face_metrics(face: &Face) -> FaceMetrics {
        let scale = 1.0 / face.units_per_em() as f32;
        let fixed_width = face.glyph_hor_advance(GlyphId(0)).unwrap_or(0) as f64;
        let fixed_height = face.height() as f64;
        FaceMetrics {
            width: 1.0,
            height: (fixed_height / fixed_width) as f32,
            descender: face.descender().abs() as f32 / fixed_height as f32,
            pixel_scale: scale as f64,
            scale_x: 1.0 / fixed_width,
            scale_y: 1.0 / fixed_height
        }
    }

    fn convert_atlas_index_to_offset(index: u32) -> [u32; 2] {
        let tex_index = index * MSDF_SIZE;
        let x = tex_index % ATLAS_SIZE;
        let y = tex_index / ATLAS_SIZE;
        [x, y * MSDF_SIZE]
    }

    fn convert_atlas_offset_to_texcoord(offset: [u32; 2]) -> [f64; 2] {
        let atlas_f64 = ATLAS_SIZE as f64;
        [offset[0] as f64 / atlas_f64, offset[1] as f64 / atlas_f64]
    }

    fn load_font_by_name(cache: &FontCache, name: &str) -> Result<Vec<u8>, String> {
        match cache.query(&FcPattern {
            name: Some(name.to_string()),
            ..Default::default()
        }) {
            Some(res) => Self::load_font_data(&res.path),
            None => Err(format!("Font '{name}' not found!"))
        }
    }

    fn load_font_from_resources(name: &str) -> Result<Vec<u8>, String> {
        let path = crate::resources::get_resource_path(name);
        Self::load_font_data(path.to_str().unwrap())
    }

    fn load_font_by_glyph(cache: &FontCache, c: char) -> Result<Vec<u8>, String> {
        // Font cache unicode range is not implemented yet so this function
        // is useless
        unimplemented!();

        match cache.query(&FcPattern {
            unicode_range: [c as usize, c as usize + 1],
            ..Default::default()
        }) {
            Some(res) => Self::load_font_data(&res.path),
            None => Err(format!("Font with char '{:?}' not found!", c))
        }
    }

    fn load_font_data(path: &str) -> Result<Vec<u8>, String> {
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(msg) => return Err(msg.to_string())
        };
        let mut reader = BufReader::new(file);
        let mut font_data = vec![];
        if let Err(msg) = reader.read_to_end(&mut font_data) {
            return Err(msg.to_string());
        }

        // Validate font file
        if let Err(msg) = Face::parse(font_data.as_slice(), 0) {
            return Err(msg.to_string());
        }

        Ok(font_data)
    }
}
