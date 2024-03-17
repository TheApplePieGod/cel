use rust_fontconfig::{FcFontCache, FcPattern};
use ttf_parser::{Face, GlyphId, RasterGlyphImage};
use msdfgen::{FontExt, Bitmap, Range, MsdfGeneratorConfig, FillRule, Bound, Shape};
use std::{fs::File, io::{BufReader, Read}, collections::{HashMap, hash_map::Entry}};
use std::num::NonZeroUsize;
use lru::LruCache;
use crate::texture::Texture;

pub type FontCache = FcFontCache;

const ATLAS_SIZE: u32 = 2048;
const MSDF_SIZE: u32 = 32;
const MSDF_RANGE: f32 = 4.0;

#[derive(Default)]
pub struct FaceMetrics {
    pub height: f32,
    pub width: f32,
    pub descender: f32,
    pub space_size: f32
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
    pub advance: f32,
    pub glyph_bound: Bound<f32>,
    pub atlas_bound: Bound<f32>,
    pub render_type: RenderType
}

pub struct GlyphData {
    pixels: Vec<f32>,
    metrics: GlyphMetrics
}

pub struct Font {
    name_list: Vec<String>,
    font_data: Vec<Vec<u8>>,
    glyph_cache: HashMap<char, GlyphData>,
    glyph_lru: LruCache<char, GlyphMetrics>,
    atlas_free_list: u32,
    atlas_tex: Texture<f32>
}

impl Default for GlyphData {
    fn default() -> Self {
        Self {
            pixels: vec![1.0; (MSDF_SIZE * MSDF_SIZE * 4) as usize],
            metrics: GlyphMetrics {
                atlas_index: 0,
                atlas_uv: Default::default(),
                advance: 0.0,
                glyph_bound: Default::default(),
                atlas_bound: Default::default(),
                render_type: RenderType::RASTER
            }
        }
    }
}

impl Font {
    pub fn new(
        cache: &FontCache,
        name_list: &Vec<&str>,
    ) -> Result<Self, String> {
        let mut font_data = vec![];
        for name in name_list {
            match Self::load_font_by_name(&cache, &name) {
                Ok(data) => font_data.push(data),
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
            name_list: name_list.iter().map(|n| n.to_string()).collect(),
            font_data,
            glyph_cache: Default::default(),
            glyph_lru: LruCache::new(NonZeroUsize::new(max_glyphs as usize).unwrap()),
            atlas_free_list: 1, // Spot zero is always empty
            atlas_tex
        })
    }

    pub fn get_primary_name(&self) -> &str {
        &self.name_list[0]
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

    pub fn get_face_metrics(&self) -> FaceMetrics {
        let face = Face::parse(self.font_data[0].as_slice(), 0).unwrap();
        let scale = face.units_per_em() as f32;
        let space_glyph = face.glyph_index(' ').unwrap();
        FaceMetrics {
            height: face.global_bounding_box().height() as f32 / scale,
            width: face.global_bounding_box().width() as f32 / scale,
            descender: face.descender().abs() as f32 / scale,
            space_size: face.glyph_hor_advance(space_glyph).unwrap() as f32 / scale
        }
    }

    pub fn get_atlas_texcoord(atlas_index: u32) -> [f32; 2] {
        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        return Self::convert_atlas_offset_to_texcoord(offset);
    }

    pub fn get_glyph_data(&mut self, key: char) -> GlyphMetrics {
        // TODO: don't need to evict if new glyph is invalid (default to index 0)
        match self.glyph_lru.get(&key) {
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

                let glyph_data = self.update_char_in_atlas(key, atlas_index);

                glyph_data.metrics
            }
        }
    }

    fn update_char_in_atlas(&mut self, key: char, atlas_index: u32) -> &GlyphData {
        // TODO: batching for face parsing 
        if !self.glyph_cache.contains_key(&key) {
            self.glyph_cache.insert(key, self.load_glyph(key));
        }

        let glyph_data = self.glyph_cache.get_mut(&key).unwrap();

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
            uv_min[0],
            uv_max[1],
            uv_max[0],
            uv_min[1],
        );

        self.glyph_lru.put(key, glyph_data.metrics);

        glyph_data
    }

    fn load_glyph(&self, key: char) -> GlyphData {
        let render_type: RenderType;
        let mut font_index = 0;
        let mut face: Face;
        let mut raster: Option<RasterGlyphImage>;
        let mut shape: Option<Shape> = None;
        let mut glyph_index: Option<GlyphId>;
        loop {
            if font_index >= self.font_data.len() {
                log::warn!("Missing glyph for '{:?}'", key);
                return Default::default();
            }

            face = Face::parse(self.font_data[font_index].as_slice(), 0).unwrap();
            glyph_index = face.glyph_index(key);
            font_index += 1;

            if glyph_index.is_some() {
                raster = face.glyph_raster_image(glyph_index.unwrap(), std::u16::MAX);
                if raster.is_some() {
                    render_type = RenderType::RASTER;
                    break;
                }
                shape = face.glyph_shape(glyph_index.unwrap());
                if shape.is_some() {
                    render_type = RenderType::MSDF;
                    break;
                }
            }
        }

        let glyph_index = glyph_index.unwrap();
        let (pixels, glyph_bound, pixel_bound) = match render_type {
            RenderType::MSDF => Self::generate_msdf(&face, shape.unwrap()),
            RenderType::RASTER => Self::generate_raster(raster.unwrap())
        };

        // Compute final atlas bound (uv coords)
        let texcoord_scale = 1.0 / self.get_atlas_size() as f32;
        let atlas_bound = Bound::new(
            pixel_bound.left * texcoord_scale,
            pixel_bound.bottom * texcoord_scale,
            pixel_bound.right * texcoord_scale,
            pixel_bound.top * texcoord_scale,
        );

        let scale = face.units_per_em() as f32;
        GlyphData {
            pixels,
            metrics: GlyphMetrics {
                atlas_index: 0,
                atlas_uv: Bound::new(0.0, 0.0, 0.0, 0.0),
                advance: face.glyph_hor_advance(glyph_index).unwrap_or(0) as f32 / scale as f32,
                glyph_bound,
                atlas_bound,
                render_type
            }
        }
    }

    fn generate_msdf(face: &Face, shape: Shape) -> (Vec<f32>, Bound<f32>, Bound<f32>) {
        let mut shape = shape;
        let bound = shape.get_bound();
        let framing = bound.autoframe(
            MSDF_SIZE,
            MSDF_SIZE,
            Range::Px(MSDF_RANGE as f64),
            None
        ).unwrap();

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

        // Convert pixel units to normalized units
        let scale = face.units_per_em() as f32;
        let glyph_bound = Bound::new(
            bound.left as f32 / scale,
            bound.bottom as f32 / scale,
            bound.right as f32 / scale,
            bound.top as f32 / scale
        );

        // Cursed math that I don't fully understand but basically this is
        // necessary to go from glyph space into pixel space within the atlas
        let pixel_bound = Bound::new(
            ((bound.left + framing.translate.x) * framing.scale.x) as f32,
            ((bound.bottom + framing.translate.y) * framing.scale.y) as f32,
            ((bound.right + framing.translate.x) * framing.scale.x) as f32,
            ((bound.top + framing.translate.y) * framing.scale.y) as f32
        );

        (pixels, glyph_bound, pixel_bound)
    }

    fn generate_raster(raster: RasterGlyphImage) -> (Vec<f32>, Bound<f32>, Bound<f32>) {
        // TODO: half pixel correction here is not great
        let glyph_bound = Bound::new(0.0, 0.0, 1.0, 1.0);
        let pixel_bound = Bound::new(0.5, 0.5, MSDF_SIZE as f32 - 0.5, MSDF_SIZE as f32 - 0.5);

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

        let pixels = image.to_rgba32f().as_raw().clone();

        (pixels, glyph_bound, pixel_bound)
    }

    fn convert_atlas_index_to_offset(index: u32) -> [u32; 2] {
        let tex_index = index * MSDF_SIZE;
        let x = tex_index % ATLAS_SIZE;
        let y = tex_index / ATLAS_SIZE;
        [x, y * MSDF_SIZE]
    }

    fn convert_atlas_offset_to_texcoord(offset: [u32; 2]) -> [f32; 2] {
        let atlas_f32 = ATLAS_SIZE as f32;
        [offset[0] as f32 / atlas_f32, offset[1] as f32 / atlas_f32]
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
